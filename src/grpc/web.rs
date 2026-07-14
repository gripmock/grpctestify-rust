use anyhow::{Context, Result, anyhow};
use prost::Message;
use prost_reflect::{DynamicMessage, MessageDescriptor, SerializeOptions};
use serde_json::Value;
use std::collections::HashMap;

use crate::grpc::{GrpcClientConfig, TransportResult, WireProtocol};

pub async fn execute_web(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    match config.protocol {
        WireProtocol::ConnectRpc => {
            connect_rpc_json(config, service_name, method_name, request_body).await
        }
        WireProtocol::GrpcWeb => grpc_web(config, service_name, method_name, request_body).await,
        _ => Err(anyhow!(
            "Unsupported protocol for HTTP transport: {:?}",
            config.protocol
        )),
    }
}

#[derive(Debug, Default)]
pub struct WebResponse {
    pub messages: Vec<Value>,
    pub trailers: HashMap<String, String>,
    pub error: Option<String>,
}

impl From<WebResponse> for TransportResult {
    fn from(r: WebResponse) -> Self {
        TransportResult {
            messages: r.messages,
            headers: HashMap::new(),
            trailers: r.trailers,
            error: r.error,
        }
    }
}

// ─── ConnectRPC (JSON, no proto schema needed) ──────────────

async fn connect_rpc_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let body =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;

    let (status, response_bytes) =
        send_http_post(config, service_name, method_name, "application/json", &body).await?;

    if !status.is_success() && response_bytes.is_empty() {
        return Err(anyhow!("HTTP {} from server", status));
    }

    let result: Value = serde_json::from_slice(&response_bytes).with_context(|| {
        format!(
            "Invalid JSON response: {}",
            String::from_utf8_lossy(&response_bytes)
        )
    })?;

    Ok(WebResponse {
        messages: vec![result],
        ..Default::default()
    })
}

// ─── gRPC-Web (binary proto or JSON) ────────────────────────

async fn grpc_web(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    if config.proto_config.is_some() {
        return grpc_web_binary(config, service_name, method_name, request_body).await;
    }
    grpc_web_json(config, service_name, method_name, request_body).await
}

/// Proper gRPC-Web JSON: `application/grpc-web+json` with 5-byte framing.
async fn grpc_web_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let json_bytes =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;

    let len = json_bytes.len() as u32;
    let mut body = Vec::with_capacity(json_bytes.len() + 5);
    body.push(0x00);
    body.extend_from_slice(&len.to_be_bytes());
    body.extend_from_slice(&json_bytes);

    let (status, response_bytes) = send_http_post(
        config,
        service_name,
        method_name,
        "application/grpc-web+json",
        &body,
    )
    .await?;

    if !status.is_success() && response_bytes.is_empty() {
        return Err(anyhow!("HTTP {} from server", status));
    }

    let (messages, trailers, error) = parse_grpc_web_framed_json(&response_bytes);
    Ok(WebResponse {
        messages,
        trailers,
        error,
    })
}

async fn grpc_web_binary(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
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
        .with_context(|| "Failed to load proto descriptors from collection")?;
    let pool = client.descriptor_pool();

    let svc = pool
        .get_service_by_name(service_name)
        .ok_or_else(|| anyhow!("Service '{}' not found in proto config", service_name))?;
    let method = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| anyhow!("Method '{}' not found in proto config", method_name))?;

    let input_desc = method.input();
    let output_desc = method.output();

    let request_bytes = serialize_message(&request_body, &input_desc)?;

    let len = request_bytes.len() as u32;
    let mut body = Vec::with_capacity(request_bytes.len() + 5);
    body.push(0x00);
    body.extend_from_slice(&len.to_be_bytes());
    body.extend_from_slice(&request_bytes);

    let (status, response_bytes) = send_http_post(
        config,
        service_name,
        method_name,
        "application/grpc-web-proto",
        &body,
    )
    .await?;

    if !status.is_success() && response_bytes.is_empty() {
        return Err(anyhow!("HTTP {} from server", status));
    }

    let messages = parse_grpc_web_framed_proto(&response_bytes, &output_desc);
    Ok(WebResponse {
        messages,
        ..Default::default()
    })
}

// ─── HTTP POST helper ───────────────────────────────────────

async fn send_http_post(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(axum::http::StatusCode, Vec<u8>)> {
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

    let mut http_req = http_client.post(&url).header("Content-Type", content_type);

    if let Some(ref metadata) = config.metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            http_req = http_req.header(k.as_str(), v.as_str());
        }
    }

    let response = http_req
        .body(body.to_vec())
        .send()
        .await
        .with_context(|| format!("Request to {} failed", url))?;

    let status = response.status();
    let response_bytes = response
        .bytes()
        .await
        .with_context(|| "Failed to read response")?;

    Ok((status, response_bytes.to_vec()))
}

// ─── Serialization helpers ──────────────────────────────────

fn serialize_message(value: &Value, desc: &MessageDescriptor) -> Result<Vec<u8>> {
    let json_str = serde_json::to_string(value)?;
    let mut deserializer = serde_json::Deserializer::from_str(&json_str);
    let msg = DynamicMessage::deserialize(desc.clone(), &mut deserializer)
        .with_context(|| "Failed to serialize JSON to protobuf")?;
    let mut buf = Vec::new();
    msg.encode(&mut buf)?;
    Ok(buf)
}

fn parse_grpc_web_framed_json(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut offset = 0;

    while offset + 5 <= data.len() {
        let flags = data[offset];
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
        let payload = &data[offset..offset + len];
        offset += len;

        if flags & 0x80 != 0 {
            // Trailer frame
            let text = String::from_utf8_lossy(payload);
            for line in text.lines() {
                if let Some((k, v)) = line.split_once(": ") {
                    trailers.insert(k.to_string(), v.to_string());
                }
            }
        } else {
            // Data frame — parse as JSON
            match serde_json::from_slice(payload) {
                Ok(val) => messages.push(val),
                Err(_) => continue,
            }
        }
    }

    if let Some(status) = trailers.get("grpc-status").filter(|s| *s != "0") {
        let msg = trailers.get("grpc-message").cloned().unwrap_or_default();
        error = Some(format!("gRPC error: code={} message={}", status, msg));
    }

    (messages, trailers, error)
}

fn parse_grpc_web_framed_proto(data: &[u8], output_desc: &MessageDescriptor) -> Vec<Value> {
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
