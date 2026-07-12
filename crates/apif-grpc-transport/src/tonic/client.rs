use anyhow::Result;
use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use prost_reflect::{DynamicMessage, Kind, MessageDescriptor, SerializeOptions};
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tonic::codec::CompressionEncoding;
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};
use tonic::transport::Uri;
use tonic::{Request, Status};

use crate::client::GrpcClient;
use crate::config::{CompressionMode, GrpcClientConfig};
use crate::error::GrpcError;
use crate::tonic::channel::create_channel;
use crate::tonic::codec::DynamicCodec;
use crate::tonic::descriptor;
use crate::types::{EndpointMeta, MethodInfo, RpcMode, StreamItem};

pub struct TonicGrpcClient {
    client: tonic::client::Grpc<tonic::transport::Channel>,
    descriptor_pool: Arc<prost_reflect::DescriptorPool>,
    config: GrpcClientConfig,
}

impl TonicGrpcClient {
    pub async fn new(config: GrpcClientConfig) -> Result<Self> {
        let channel = create_channel(&config).await?;
        let descriptor_pool = descriptor::load_descriptors(&config).await?;
        let mut client = tonic::client::Grpc::new(channel);
        if config.compression == CompressionMode::Gzip {
            client = client.send_compressed(CompressionEncoding::Gzip);
            client = client.accept_compressed(CompressionEncoding::Gzip);
        }
        Ok(Self {
            client,
            descriptor_pool,
            config,
        })
    }
    pub fn descriptor_pool(&self) -> &prost_reflect::DescriptorPool {
        &self.descriptor_pool
    }
}

#[async_trait]
impl GrpcClient for TonicGrpcClient {
    async fn call_stream(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: Pin<Box<dyn Stream<Item = Value> + Send>>,
    ) -> Result<
        (
            HashMap<String, String>,
            Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>>,
        ),
        GrpcError,
    > {
        let svc = self
            .descriptor_pool
            .get_service_by_name(service_name)
            .ok_or_else(|| GrpcError::new(5, format!("Service '{}' not found", service_name)))?;
        let m = svc
            .methods()
            .find(|m| m.name() == method_name)
            .ok_or_else(|| GrpcError::new(5, format!("Method '{}' not found", method_name)))?;
        let input_desc = m.input();
        let path = format!("/{}/{}", service_name, method_name)
            .parse::<Uri>()
            .map_err(|e| GrpcError::new(3, format!("Invalid path: {}", e)))?
            .path_and_query()
            .ok_or_else(|| GrpcError::new(3, "Invalid path"))?
            .clone();

        let mut requests = requests; // shadow for mutability
        let codec = DynamicCodec::new(m.input(), m.output());
        let mut client = self.client.clone();
        let _ = client.ready().await;

        let is_cs = m.is_client_streaming();
        let is_ss = m.is_server_streaming();

        let result: Result<
            (
                HashMap<String, String>,
                Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>>,
            ),
            GrpcError,
        >;

        if is_cs {
            // Client-streaming or bidi: wrap all values via filter_map
            let input_clone = input_desc.clone();
            let req_stream = Box::pin(requests.filter_map(move |json| {
                let desc = input_clone.clone();
                async move {
                    let mut v = json;
                    transform_input_json_for_well_known(&mut v, &desc);
                    serde_json::to_string(&v).ok().and_then(|s| {
                        DynamicMessage::deserialize(
                            desc,
                            &mut serde_json::Deserializer::from_str(&s),
                        )
                        .ok()
                    })
                }
            }));
            let mut req = Request::new(req_stream);
            insert_metadata(req.metadata_mut(), self.config.metadata.as_ref());
            result = if is_ss {
                client
                    .streaming(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let s: Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>> =
                            Box::pin(r.into_inner().map(|item| {
                                item.map(|msg| StreamItem::Message(dynamic_message_to_json(&msg)))
                                    .map_err(|s| tonic_status_to_grpc_error(&s))
                            }));
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            } else {
                client
                    .client_streaming(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let val = dynamic_message_to_json(&r.into_inner());
                        let s: Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>> =
                            Box::pin(futures::stream::once(async move {
                                Ok(StreamItem::Message(val))
                            }));
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            };
        } else {
            // Unary or server-streaming: get first value directly (bypass filter_map)
            let json_val = requests
                .next()
                .await
                .ok_or_else(|| GrpcError::new(3, "Missing request message".to_string()))?;
            let mut v = json_val;
            transform_input_json_for_well_known(&mut v, &input_desc);
            let json_str = serde_json::to_string(&v)
                .map_err(|e| GrpcError::new(3, format!("JSON error: {}", e)))?;
            let msg = DynamicMessage::deserialize(
                input_desc.clone(),
                &mut serde_json::Deserializer::from_str(&json_str),
            )
            .map_err(|e| GrpcError::new(3, format!("Deser error: {}", e)))?;
            let mut req = Request::new(msg);
            insert_metadata(req.metadata_mut(), self.config.metadata.as_ref());
            result = if is_ss {
                client
                    .server_streaming(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let s: Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>> =
                            Box::pin(r.into_inner().map(|item| {
                                item.map(|msg| StreamItem::Message(dynamic_message_to_json(&msg)))
                                    .map_err(|s| tonic_status_to_grpc_error(&s))
                            }));
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            } else {
                client
                    .unary(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let val = dynamic_message_to_json(&r.into_inner());
                        let s: Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>> =
                            Box::pin(futures::stream::once(async move {
                                Ok(StreamItem::Message(val))
                            }));
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            };
        }

        result
    }

    fn list_services(&self) -> Vec<String> {
        self.descriptor_pool
            .services()
            .map(|s| s.full_name().to_string())
            .collect()
    }
    fn list_methods(&self, service_name: &str) -> Vec<MethodInfo> {
        self.descriptor_pool
            .get_service_by_name(service_name)
            .map(|svc| {
                svc.methods()
                    .map(|m| MethodInfo {
                        name: m.name().to_string(),
                        full_name: format!("{}/{}", svc.full_name(), m.name()),
                        input_type: m.input().full_name().to_string(),
                        output_type: m.output().full_name().to_string(),
                        client_streaming: m.is_client_streaming(),
                        server_streaming: m.is_server_streaming(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta, GrpcError> {
        let p: Vec<&str> = endpoint.split('/').collect();
        if p.len() != 2 {
            return Err(GrpcError::new(3, format!("Invalid endpoint: {}", endpoint)));
        }
        let svc = self
            .descriptor_pool
            .get_service_by_name(p[0])
            .ok_or_else(|| GrpcError::new(5, format!("Service not found: {}", p[0])))?;
        let m = svc
            .methods()
            .find(|m| m.name() == p[1])
            .ok_or_else(|| GrpcError::new(5, format!("Method not found: {}", p[1])))?;
        Ok(EndpointMeta {
            rpc_mode: match (m.is_client_streaming(), m.is_server_streaming()) {
                (false, false) => RpcMode::Unary,
                (false, true) => RpcMode::ServerStream,
                (true, false) => RpcMode::ClientStream,
                (true, true) => RpcMode::Bidi,
            },
            input_type: m.input().full_name().to_string(),
            output_type: m.output().full_name().to_string(),
        })
    }
    fn generate_schema(&self, endpoint: &str) -> Result<Value, GrpcError> {
        let p: Vec<&str> = endpoint.split('/').collect();
        if p.len() != 2 {
            return Err(GrpcError::new(3, format!("Invalid endpoint: {}", endpoint)));
        }
        let svc = self
            .descriptor_pool
            .get_service_by_name(p[0])
            .ok_or_else(|| GrpcError::new(5, format!("Service not found: {}", p[0])))?;
        let m = svc
            .methods()
            .find(|m| m.name() == p[1])
            .ok_or_else(|| GrpcError::new(5, format!("Method not found: {}", p[1])))?;
        Ok(generate_json_template(&m.input()))
    }
}

fn insert_metadata(meta: &mut MetadataMap, custom: Option<&HashMap<String, String>>) {
    let default_ua = format!("grpctestify/{}", env!("CARGO_PKG_VERSION"));
    let ua = custom
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
                .map(|(_, v)| v.as_str())
        })
        .unwrap_or(&default_ua);
    if let Ok(val) = MetadataValue::from_str(ua) {
        meta.insert("user-agent", val);
    }
    if let Some(md) = custom {
        for (k, v) in md {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            if let Ok(key) = MetadataKey::from_str(&k.to_ascii_lowercase())
                && let Ok(val) = MetadataValue::from_str(v)
            {
                meta.insert(key, val);
            }
        }
    }
}

fn metadata_map_to_hashmap(metadata: &MetadataMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for e in metadata.iter() {
        match e {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => {
                map.insert(k.to_string(), v.to_str().unwrap_or("?").to_string());
            }
            tonic::metadata::KeyAndValueRef::Binary(k, v) => {
                map.insert(
                    k.to_string(),
                    v.to_bytes().iter().map(|b| format!("{:02x}", b)).collect(),
                );
            }
        }
    }
    map
}

fn tonic_status_to_grpc_error(status: &Status) -> GrpcError {
    let metadata = status
        .metadata()
        .iter()
        .filter_map(|e| match e {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => {
                Some((k.to_string(), v.to_str().unwrap_or("?").to_string()))
            }
            _ => None,
        })
        .collect();
    GrpcError::with_metadata(
        status.code() as u32,
        status.message(),
        status.details().to_vec(),
        metadata,
    )
}

fn dynamic_message_to_json(msg: &DynamicMessage) -> Value {
    msg.serialize_with_options(
        serde_json::value::Serializer,
        &SerializeOptions::new().use_proto_field_name(true),
    )
    .unwrap_or(Value::Null)
}

fn transform_input_json_for_well_known(value: &mut Value, desc: &MessageDescriptor) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    for key in obj.keys().cloned().collect::<Vec<_>>() {
        let Some(field) = desc
            .get_field_by_json_name(&key)
            .or_else(|| desc.get_field_by_name(&key))
        else {
            continue;
        };
        let Some(fv) = obj.get_mut(&key) else {
            continue;
        };
        if let Kind::Message(md) = field.kind() {
            if md.full_name() == "google.protobuf.FieldMask"
                && let Some(paths) = fv
                    .as_object()
                    .and_then(|m| m.get("paths"))
                    .and_then(|v| v.as_array())
            {
                *fv = Value::String(
                    paths
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                );
                continue;
            }
            if field.is_list() {
                if let Some(arr) = fv.as_array_mut() {
                    for item in arr {
                        transform_input_json_for_well_known(item, &md);
                    }
                }
            } else {
                transform_input_json_for_well_known(fv, &md);
            }
        }
    }
}

fn generate_json_template(desc: &prost_reflect::MessageDescriptor) -> Value {
    let mut obj = serde_json::Map::new();
    for field in desc.fields() {
        let name = field.json_name().to_string();
        let fv = fake_value(&name, &field.kind());
        if field.is_list() {
            obj.insert(name, Value::Array(vec![fv]));
        } else if field.is_map() {
            obj.insert(name, Value::Object(serde_json::Map::new()));
        } else {
            obj.insert(name, fv);
        }
    }
    Value::Object(obj)
}

fn fake_value(field_name: &str, kind: &prost_reflect::Kind) -> Value {
    let n = rand::random::<u32>() % 100000;
    match kind {
        Kind::Double | Kind::Float => serde_json::json!((n as f64) / 10.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => serde_json::json!(n as i32),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => serde_json::json!((n * 100) as i64),
        Kind::Uint32 | Kind::Fixed32 => serde_json::json!(n),
        Kind::Uint64 | Kind::Fixed64 => serde_json::json!((n * 100) as u64),
        Kind::Bool => serde_json::json!(n.is_multiple_of(2)),
        Kind::String => Value::String(fake_string(field_name)),
        Kind::Bytes => Value::String(format!("{} bytes", n)),
        Kind::Enum(ed) => Value::String(
            ed.values()
                .next()
                .map(|v| v.name().to_string())
                .unwrap_or("UNSPECIFIED".into()),
        ),
        Kind::Message(md) => {
            let f = md.full_name();
            if f == "google.protobuf.Timestamp" {
                serde_json::json!("2024-06-15T10:30:00Z")
            } else if f == "google.protobuf.Duration" {
                serde_json::json!("30s")
            } else if f == "google.protobuf.FieldMask" {
                serde_json::json!({"paths": ["field"]})
            } else if f == "google.protobuf.Struct" {
                serde_json::json!({"key": "value"})
            } else if f == "google.protobuf.Value" {
                serde_json::json!("value")
            } else if f == "google.protobuf.Any" {
                serde_json::json!({"@type": "type.googleapis.com/example.Msg", "field": "v"})
            } else {
                generate_json_template(md)
            }
        }
    }
}

fn fake_string(field_name: &str) -> String {
    let l = field_name.to_lowercase();
    if l.contains("email") {
        "user@example.com"
    } else if l.contains("first") && l.contains("name") {
        "John"
    } else if l.contains("last") && l.contains("name") {
        "Doe"
    } else if l.contains("name") {
        "John Doe"
    } else if l.contains("phone") {
        "+1-555-123-4567"
    } else if l.contains("url") {
        "https://example.com"
    } else if l.contains("uuid") {
        "550e8400-e29b-41d4-a716-446655440000"
    } else if l.contains("addr") {
        "123 Main St"
    } else if l.contains("city") {
        "New York"
    } else if l.contains("country") {
        "US"
    } else if l.contains("zip") || l.contains("post") {
        "10001"
    } else if l.contains("password") {
        "••••••••"
    } else if l.contains("token") {
        "tok_abc"
    } else if l.contains("desc") || l.contains("comment") {
        "A description."
    } else if l.contains("status") {
        "active"
    } else if l.contains("type") || l.contains("kind") {
        "standard"
    } else if l.contains("date") || l.contains("time") {
        "2024-06-15T10:30:00Z"
    } else if l.contains("color") {
        "#3b82f6"
    } else if l.contains("lang") {
        "en-US"
    } else if l.contains("title") {
        "Title"
    } else if l.contains("company") {
        "Acme Corp"
    } else if l.contains("job") {
        "Engineer"
    } else {
        "sample"
    }
    .to_string()
}
