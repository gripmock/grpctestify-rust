#![allow(clippy::unwrap_used, clippy::expect_used)]
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

#[derive(Debug)]
pub struct TonicGrpcClient {
    client: tonic::client::Grpc<tonic::transport::Channel>,
    descriptor_pool: Arc<prost_reflect::DescriptorPool>,
    config: GrpcClientConfig,
}

impl TonicGrpcClient {
    pub async fn new(config: GrpcClientConfig) -> Result<Self> {
        anyhow::ensure!(
            config.protocol == crate::config::WireProtocol::Grpc,
            "tonic transport only supports gRPC protocol, not {:?}",
            config.protocol,
        );
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

        let mut requests = requests;
        let codec = DynamicCodec::new(m.input(), m.output());
        let mut client = self.client.clone();
        client
            .ready()
            .await
            .map_err(|e| GrpcError::new(14, format!("Failed to establish connection: {}", e)))?;

        let is_cs = m.is_client_streaming();
        let is_ss = m.is_server_streaming();

        let mut result: Result<
            (
                HashMap<String, String>,
                Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>>,
            ),
            GrpcError,
        >;

        if is_cs {
            let input_clone = input_desc.clone();
            let conversion_error: ConversionErrorSlot = Arc::new(std::sync::Mutex::new(None));
            let req_stream = Box::pin(requests.enumerate().scan(
                conversion_error.clone(),
                move |slot, (index, json)| {
                    let desc = input_clone.clone();
                    let slot = slot.clone();
                    async move {
                        match convert_request_json(index, json, &desc) {
                            Ok(msg) => Some(msg),
                            Err(e) => {
                                *slot.lock().unwrap() = Some(e);
                                None
                            }
                        }
                    }
                },
            ));
            let mut req = Request::new(req_stream);
            insert_metadata(
                req.metadata_mut(),
                self.config.metadata.as_ref(),
                &self.config.version,
            );
            result = if is_ss {
                client
                    .streaming(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let s = streaming_response_to_items(
                            r.into_inner(),
                            Some(conversion_error.clone()),
                        );
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            } else {
                match client.streaming(req, path, codec).await {
                    Ok(r) => single_response_to_items(r).await,
                    Err(s) => Err(tonic_status_to_grpc_error(&s)),
                }
            };
            if let Some(err) = conversion_error.lock().unwrap().take() {
                result = Err(err);
            }
        } else {
            let json_val = requests
                .next()
                .await
                .ok_or_else(|| GrpcError::new(3, "Missing request message".to_string()))?;
            let msg = convert_request_json(0, json_val, &input_desc)?;
            result = if is_ss {
                let mut req = Request::new(msg);
                insert_metadata(
                    req.metadata_mut(),
                    self.config.metadata.as_ref(),
                    &self.config.version,
                );
                client
                    .server_streaming(req, path, codec)
                    .await
                    .map(|r| {
                        let h = metadata_map_to_hashmap(r.metadata());
                        let s = streaming_response_to_items(r.into_inner(), None);
                        (h, s)
                    })
                    .map_err(|s| tonic_status_to_grpc_error(&s))
            } else {
                let mut req = Request::new(futures::stream::iter(vec![msg]));
                insert_metadata(
                    req.metadata_mut(),
                    self.config.metadata.as_ref(),
                    &self.config.version,
                );
                match client.streaming(req, path, codec).await {
                    Ok(r) => single_response_to_items(r).await,
                    Err(s) => Err(tonic_status_to_grpc_error(&s)),
                }
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
}

type ItemStream = Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>>;
type ConversionErrorSlot = Arc<std::sync::Mutex<Option<GrpcError>>>;

fn convert_request_json(
    index: usize,
    mut json: Value,
    desc: &MessageDescriptor,
) -> Result<DynamicMessage, GrpcError> {
    transform_input_json_for_well_known(&mut json, desc);
    DynamicMessage::deserialize(desc.clone(), json).map_err(|e| {
        GrpcError::new(
            3,
            format!(
                "Failed to convert request message #{} to protobuf: {}",
                index, e
            ),
        )
    })
}

fn streaming_response_to_items(
    body: tonic::Streaming<DynamicMessage>,
    conversion_error: Option<ConversionErrorSlot>,
) -> ItemStream {
    enum Phase {
        Messages(Box<tonic::Streaming<DynamicMessage>>),
        Done,
    }
    let items = futures::stream::unfold(Phase::Messages(Box::new(body)), |phase| async move {
        match phase {
            Phase::Messages(mut body) => match body.message().await {
                Ok(Some(msg)) => Some((
                    Ok(StreamItem::Message(dynamic_message_to_json(&msg))),
                    Phase::Messages(body),
                )),
                Ok(None) => match body.trailers().await {
                    Ok(Some(trailers)) => {
                        let map = metadata_map_to_hashmap(&trailers);
                        if map.is_empty() {
                            None
                        } else {
                            Some((Ok(StreamItem::Trailers(map)), Phase::Done))
                        }
                    }
                    Ok(None) => None,
                    Err(status) => Some((Err(tonic_status_to_grpc_error(&status)), Phase::Done)),
                },
                Err(status) => Some((
                    Err(tonic_status_to_grpc_error(&status)),
                    Phase::Messages(body),
                )),
            },
            Phase::Done => None,
        }
    });
    match conversion_error {
        Some(slot) => {
            let tail = futures::stream::unfold(slot, |slot| async move {
                let err = slot.lock().unwrap().take();
                err.map(|e| (Err(e), slot))
            });
            Box::pin(items.fuse().chain(tail.fuse()).fuse())
        }
        None => Box::pin(items.fuse()),
    }
}

async fn single_response_to_items(
    response: tonic::Response<tonic::Streaming<DynamicMessage>>,
) -> Result<(HashMap<String, String>, ItemStream), GrpcError> {
    let headers = metadata_map_to_hashmap(response.metadata());
    let mut body = response.into_inner();
    let first = match body.message().await {
        Ok(Some(msg)) => msg,
        Ok(None) => return Err(GrpcError::new(13, "Missing response message.")),
        Err(status) => {
            let mut err = tonic_status_to_grpc_error(&status);
            for (k, v) in &headers {
                err.metadata.entry(k.clone()).or_insert_with(|| v.clone());
            }
            return Err(err);
        }
    };
    let val = dynamic_message_to_json(&first);
    let trailers = match body.trailers().await {
        Ok(Some(t)) => metadata_map_to_hashmap(&t),
        Ok(None) => HashMap::new(),
        Err(status) => return Err(tonic_status_to_grpc_error(&status)),
    };
    let mut items: Vec<Result<StreamItem, GrpcError>> = vec![Ok(StreamItem::Message(val))];
    if !trailers.is_empty() {
        items.push(Ok(StreamItem::Trailers(trailers)));
    }
    Ok((headers, Box::pin(futures::stream::iter(items))))
}

fn insert_metadata(
    meta: &mut MetadataMap,
    custom: Option<&HashMap<String, String>>,
    version: &str,
) {
    static DEFAULT_UA: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let default_ua = DEFAULT_UA.get_or_init(|| format!("grpctestify/{version}"));
    let ua = custom
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
                .map(|(_, v)| v.as_str())
        })
        .unwrap_or(default_ua);
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
    if !desc.fields().any(|f| matches!(f.kind(), Kind::Message(_))) {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WireProtocol;

    #[tokio::test]
    async fn rejects_non_grpc_protocols() {
        for proto in &[WireProtocol::GrpcWeb, WireProtocol::ConnectRpc] {
            let config = GrpcClientConfig {
                address: "localhost:4770".to_string(),
                protocol: *proto,
                ..Default::default()
            };
            let result = TonicGrpcClient::new(config).await;
            assert!(result.is_err(), "Expected error for {:?}", proto);
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("tonic transport only supports gRPC protocol"),
                "Unexpected error for {:?}: {}",
                proto,
                err
            );
        }
    }

    #[test]
    fn convert_request_json_ok() {
        use prost_reflect::ReflectMessage;
        let desc = prost_types::Timestamp::default().descriptor();
        let result = convert_request_json(0, serde_json::json!("2024-06-15T10:30:00Z"), &desc);
        assert!(
            result.is_ok(),
            "valid timestamp should convert: {:?}",
            result.err()
        );
    }

    #[test]
    fn convert_request_json_error_names_message_index() {
        use prost_reflect::ReflectMessage;
        let desc = prost_types::Timestamp::default().descriptor();
        let result = convert_request_json(2, serde_json::json!({"bogus": true}), &desc);
        let err = result.expect_err("invalid message must fail, not be dropped");
        assert_eq!(err.code, 3);
        assert!(
            err.message.contains("request message #2"),
            "error should name the bad message index: {}",
            err.message
        );
    }

    #[test]
    fn tonic_status_to_grpc_error_carries_code_message_details() {
        use tonic::{Code, Status};
        let details = prost::bytes::Bytes::from_static(&[0x08, 0x05, 0x12, 0x02, 0x68, 0x69]);
        let msg = "boom: code=42 message=nested details=[x]";
        let status = Status::with_details(Code::NotFound, msg, details.clone());
        let e = tonic_status_to_grpc_error(&status);
        assert_eq!(e.code, 5, "NotFound -> 5");
        assert_eq!(e.message, msg, "message survives verbatim");
        assert_eq!(
            e.details,
            details.to_vec(),
            "proto detail bytes carried as-is"
        );
    }

    #[test]
    fn insert_metadata_version() {
        let mut meta = MetadataMap::new();
        insert_metadata(&mut meta, None, "test-version");
        let ua = meta.get("user-agent").unwrap();
        assert_eq!(ua.to_str().unwrap(), "grpctestify/test-version");
    }

    #[test]
    fn insert_metadata_with_custom_user_agent() {
        let mut meta = MetadataMap::new();
        let mut custom = HashMap::new();
        custom.insert("user-agent".to_string(), "custom-ua/2.0".to_string());
        custom.insert("x-custom".to_string(), "value1".to_string());
        insert_metadata(&mut meta, Some(&custom), "test-version");
        assert_eq!(
            meta.get("user-agent").unwrap().to_str().unwrap(),
            "custom-ua/2.0"
        );
        assert_eq!(meta.get("x-custom").unwrap().to_str().unwrap(), "value1");
    }
}

#[cfg(test)]
mod well_known_tests {
    use super::*;

    fn pool_from(proto: &str) -> prost_reflect::DescriptorPool {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.proto");
        std::fs::write(&path, proto).expect("write proto");
        crate::tonic::descriptor::load_from_proto_files(
            &[path.to_string_lossy().to_string()],
            &[dir.path().to_string_lossy().to_string()],
        )
        .expect("compile proto")
    }

    #[test]
    fn a_field_mask_is_still_flattened() {
        let pool = pool_from(
            "syntax = \"proto3\";\npackage t;\nimport \"google/protobuf/field_mask.proto\";\nmessage M { google.protobuf.FieldMask mask = 1; }\n",
        );
        let desc = pool.get_message_by_name("t.M").expect("message");
        let mut value = serde_json::json!({"mask": {"paths": ["a", "b"]}});
        transform_input_json_for_well_known(&mut value, &desc);
        assert_eq!(value["mask"], serde_json::json!("a,b"));
    }

    #[test]
    fn a_scalar_only_message_is_left_alone() {
        let pool = pool_from(
            "syntax = \"proto3\";\npackage t;\nmessage S { string name = 1; int32 n = 2; }\n",
        );
        let desc = pool.get_message_by_name("t.S").expect("message");
        let before = serde_json::json!({"name": "x", "n": 1});
        let mut value = before.clone();
        transform_input_json_for_well_known(&mut value, &desc);
        assert_eq!(value, before);
    }
}
