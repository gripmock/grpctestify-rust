use anyhow::{Context, Result, anyhow};
use prost::Message;
use prost_reflect::{DynamicMessage, MessageDescriptor, SerializeOptions};
use serde_json::Value;
use std::pin::Pin;

use crate::grpc::{GrpcClientConfig, WireProtocol};

/// Execute a unary gRPC request over HTTP (gRPC-Web or ConnectRPC).
/// Returns a stream of response messages.
pub async fn call_unary(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_value: Value,
) -> Result<Pin<Box<dyn futures::Stream<Item = Result<Value, String>> + Send>>> {
    let path = format!("/{}/{}", service_name, method_name);

    let scheme = if config.tls_config.is_some() {
        "https"
    } else {
        "http"
    };
    let url = if config.address.starts_with("http://") || config.address.starts_with("https://") {
        format!("{}{}", config.address, path)
    } else {
        format!("{}://{}{}", scheme, config.address, path)
    };

    // Build reqwest client
    let user_agent = format!("grpctestify/{}", env!("CARGO_PKG_VERSION"));
    let mut req_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            config.timeout_seconds.max(5),
        ))
        .connect_timeout(std::time::Duration::from_secs(5))
        .user_agent(&user_agent);

    if let Some(ref tls) = config.tls_config
        && tls.insecure_skip_verify
    {
        req_builder = req_builder.danger_accept_invalid_certs(true);
    }

    let http_client = req_builder
        .build()
        .with_context(|| "Failed to build HTTP client")?;

    // For ConnectRPC — use JSON format (no proto schema needed)
    if config.protocol == WireProtocol::ConnectRpc {
        return call_connect_json(&http_client, &url, config, request_value).await;
    }

    // gRPC-Web — load descriptors and use binary protobuf
    let grpc_config = GrpcClientConfig {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        proto_config: config.proto_config.clone(),
        metadata: None,
        target_service: Some(service_name.to_string()),
        compression: Default::default(),
        connection_id: 0,
        protocol: WireProtocol::Grpc,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let client = crate::grpc::GrpcClient::new(grpc_config)
        .await
        .with_context(|| "Failed to load descriptors")?;
    let pool = client.descriptor_pool().clone();

    let svc = pool
        .get_service_by_name(service_name)
        .ok_or_else(|| anyhow!("Service '{}' not found", service_name))?;
    let method = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| anyhow!("Method '{}' not found", method_name))?;

    let input_desc = method.input();
    let output_desc = method.output();

    let request_bytes = serialize_message(&request_value, &input_desc)?;

    let content_type = "application/grpc-web-proto";
    let len = request_bytes.len() as u32;
    let mut body = Vec::with_capacity(request_bytes.len() + 5);
    body.push(0x00);
    body.extend_from_slice(&len.to_be_bytes());
    body.extend_from_slice(&request_bytes);

    let mut http_req = http_client
        .post(&url)
        .header("Content-Type", content_type)
        .header("TE", "trailers");

    if let Some(ref metadata) = config.metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            http_req = http_req.header(k.as_str(), v.as_str());
        }
    }

    let response = http_req
        .body(body)
        .send()
        .await
        .with_context(|| format!("Request to {} failed", url))?;

    let status = response.status();
    let response_bytes = response
        .bytes()
        .await
        .with_context(|| "Failed to read response")?;

    if !status.is_success() && response_bytes.is_empty() {
        return Err(anyhow!("HTTP {} from server", status));
    }

    let messages = parse_grpc_web_response(&response_bytes, &output_desc);

    Ok(Box::pin(futures::stream::iter(
        messages.into_iter().map(Ok),
    )))
}

/// ConnectRPC with JSON format — no proto schema required.
async fn call_connect_json(
    http_client: &reqwest::Client,
    url: &str,
    config: &GrpcClientConfig,
    request_value: Value,
) -> Result<Pin<Box<dyn futures::Stream<Item = Result<Value, String>> + Send>>> {
    let body =
        serde_json::to_vec(&request_value).with_context(|| "Failed to serialize request body")?;

    let mut http_req = http_client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json");

    if let Some(ref metadata) = config.metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            http_req = http_req.header(k.as_str(), v.as_str());
        }
    }

    let response = http_req
        .body(body)
        .send()
        .await
        .with_context(|| format!("Request to {} failed", url))?;

    let status = response.status();
    let response_bytes = response
        .bytes()
        .await
        .with_context(|| "Failed to read response")?;

    if !status.is_success() && response_bytes.is_empty() {
        return Err(anyhow!("HTTP {} from server", status));
    }

    let result: Value = serde_json::from_slice(&response_bytes).with_context(|| {
        let text = String::from_utf8_lossy(&response_bytes);
        format!("Failed to parse response as JSON: {}", text)
    })?;

    Ok(Box::pin(futures::stream::iter(vec![Ok(result)])))
}

fn serialize_message(value: &Value, desc: &MessageDescriptor) -> Result<Vec<u8>> {
    let json_str = serde_json::to_string(value)?;
    let mut deserializer = serde_json::Deserializer::from_str(&json_str);
    let msg = DynamicMessage::deserialize(desc.clone(), &mut deserializer)
        .with_context(|| "Failed to serialize JSON to protobuf")?;
    let mut buf = Vec::new();
    msg.encode(&mut buf)?;
    Ok(buf)
}

fn parse_grpc_web_response(data: &[u8], output_desc: &MessageDescriptor) -> Vec<Value> {
    let mut msgs = Vec::new();
    let mut offset = 0;
    while offset + 5 <= data.len() {
        let _flags = data[offset];
        let len = u32::from_be_bytes([
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
        ]) as usize;
        offset += 5;
        if offset + len > data.len() {
            break;
        }
        let msg_bytes = &data[offset..offset + len];
        if _flags & 0x80 == 0
            && let Ok(msg) = DynamicMessage::decode(output_desc.clone(), msg_bytes)
        {
            msgs.push(dynamic_message_to_json(&msg));
        }
        offset += len;
    }
    if msgs.is_empty()
        && !data.is_empty()
        && let Ok(msg) = DynamicMessage::decode(output_desc.clone(), data)
    {
        msgs.push(dynamic_message_to_json(&msg));
    }
    msgs
}

fn dynamic_message_to_json(msg: &DynamicMessage) -> Value {
    let options = SerializeOptions::new().use_proto_field_name(true);
    msg.serialize_with_options(serde_json::value::Serializer, &options)
        .unwrap_or(Value::Null)
}
