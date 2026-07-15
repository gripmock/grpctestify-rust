use anyhow::{Context, Result, anyhow};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, SerializeOptions};
use serde_json::Value;
use std::collections::HashMap;

type ResponseHeaders = HashMap<String, String>;

fn extract_headers(headers: &reqwest::header::HeaderMap) -> ResponseHeaders {
    let mut map = HashMap::new();
    for (k, v) in headers {
        if let Ok(val) = v.to_str() {
            map.insert(k.as_str().to_ascii_lowercase(), val.to_string());
        }
    }
    map
}

use crate::grpc::{GrpcClientConfig, RpcMode, TransportResult, WireProtocol};

pub async fn execute_web(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    execute_web_with_mode(config, service_name, method_name, request_body, None).await
}

pub async fn execute_web_with_mode(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    rpc_mode: Option<RpcMode>,
) -> Result<WebResponse> {
    match config.protocol {
        WireProtocol::ConnectRpc => {
            connect_rpc(config, service_name, method_name, request_body, rpc_mode).await
        }
        WireProtocol::GrpcWeb => {
            grpc_web(config, service_name, method_name, request_body, rpc_mode).await
        }
        _ => Err(anyhow!(
            "Unsupported protocol for HTTP transport: {:?}",
            config.protocol
        )),
    }
}

#[derive(Debug, Default)]
pub struct WebResponse {
    pub messages: Vec<Value>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub error: Option<String>,
}

impl From<WebResponse> for TransportResult {
    fn from(r: WebResponse) -> Self {
        TransportResult {
            messages: r.messages,
            headers: r.headers,
            trailers: r.trailers,
            error: r.error,
        }
    }
}

struct ResolvedMethod {
    rpc_mode: RpcMode,
    input_desc: MessageDescriptor,
    output_desc: MessageDescriptor,
}

async fn resolve_method(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
) -> Result<ResolvedMethod> {
    let pool = load_descriptor_pool(config)?;
    let svc = pool
        .get_service_by_name(service_name)
        .ok_or_else(|| anyhow!("Service '{}' not found", service_name))?;
    let method = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| anyhow!("Method '{}' not found", method_name))?;
    Ok(ResolvedMethod {
        rpc_mode: match (method.is_client_streaming(), method.is_server_streaming()) {
            (false, false) => RpcMode::Unary,
            (true, false) => RpcMode::ClientStream,
            (false, true) => RpcMode::ServerStream,
            (true, true) => RpcMode::Bidi,
        },
        input_desc: method.input(),
        output_desc: method.output(),
    })
}

/// Load proto descriptor pool from local config files only.
/// No network connection is made — avoids hanging on HTTP ports.
fn load_descriptor_pool(config: &GrpcClientConfig) -> Result<DescriptorPool> {
    let desc_path = config
        .proto_config
        .as_ref()
        .and_then(|p| p.descriptor.as_ref())
        .ok_or_else(|| anyhow!("No proto descriptor configured"))?;
    let desc_bytes = std::fs::read(desc_path)
        .with_context(|| format!("Failed to read descriptor file: {}", desc_path))?;
    let fds = prost_types::FileDescriptorSet::decode(&*desc_bytes)
        .with_context(|| "Failed to decode FileDescriptorSet")?;
    DescriptorPool::from_file_descriptor_set(fds).with_context(|| "Failed to build descriptor pool")
}

// ─── ConnectRPC ─────────────────────────────────────────────

async fn connect_rpc(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    rpc_mode: Option<RpcMode>,
) -> Result<WebResponse> {
    let needs_proto = config.proto_config.is_some();
    let resolved = if needs_proto {
        Some(resolve_method(config, service_name, method_name).await?)
    } else {
        None
    };

    let mode = rpc_mode.unwrap_or_else(|| {
        resolved
            .as_ref()
            .map(|r| r.rpc_mode)
            .unwrap_or(RpcMode::Unary)
    });

    match mode {
        RpcMode::Unary | RpcMode::ClientStream => {
            if let Some(ref m) = resolved {
                connect_rpc_unary_proto(
                    config,
                    service_name,
                    method_name,
                    request_body,
                    &m.input_desc,
                    &m.output_desc,
                )
                .await
            } else {
                connect_rpc_unary_json(config, service_name, method_name, request_body).await
            }
        }
        RpcMode::ServerStream | RpcMode::Bidi => {
            if let Some(ref m) = resolved {
                connect_rpc_stream_proto(
                    config,
                    service_name,
                    method_name,
                    request_body,
                    &m.input_desc,
                    &m.output_desc,
                )
                .await
            } else {
                connect_rpc_stream_json(config, service_name, method_name, request_body).await
            }
        }
    }
}

// ─── ConnectRPC (JSON, no proto schema needed) ──────────────

async fn connect_rpc_unary_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let body =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;

    let (status, response_bytes, headers) =
        send_http_post(config, service_name, method_name, "application/json", &body).await?;

    if !status.is_success() {
        let response_headers: HashMap<String, String> = headers
            .into_iter()
            .filter(|(k, _)| {
                !k.starts_with("grpc-") && *k != "content-type" && *k != "content-length"
            })
            .collect();
        if response_bytes.is_empty() {
            return Ok(WebResponse {
                error: Some(format!("HTTP {} from server", status)),
                headers: response_headers,
                ..Default::default()
            });
        }
        if let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes) {
            let code = err_body
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");
            let msg = err_body
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let details = err_body
                .get("details")
                .filter(|d| d.is_array())
                .map(|d| d.to_string())
                .unwrap_or_default();
            return Ok(WebResponse {
                error: Some(format!(
                    "gRPC error: code={} message={} details=[{}]",
                    code, msg, details
                )),
                headers: response_headers,
                ..Default::default()
            });
        }
        return Ok(WebResponse {
            error: Some(format!(
                "HTTP {}: {}",
                status,
                String::from_utf8_lossy(&response_bytes)
            )),
            headers: response_headers,
            ..Default::default()
        });
    }

    // Try parsing as plain JSON first; fall back to Connect envelope framing
    // if the server treated our unframed request as streaming.
    match serde_json::from_slice::<Value>(&response_bytes) {
        Ok(v) => {
            let trailers = HashMap::new();
            let mut error = None;
            if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
                let msg = headers.get("grpc-message").cloned().unwrap_or_default();
                error = Some(format!("gRPC error: code={} message={}", grpc_status, msg));
            }
            let response_headers: HashMap<String, String> = headers
                .into_iter()
                .filter(|(k, _)| {
                    !k.starts_with("grpc-") && *k != "content-type" && *k != "content-length"
                })
                .collect();
            return Ok(WebResponse {
                messages: vec![v],
                headers: response_headers,
                trailers,
                error,
            });
        }
        Err(_) => {
            // Not plain JSON — might be a Connect envelope response (server treated
            // our unframed request as streaming). Try parsing as framed response.
            let (messages, trailers, error) = parse_connect_framed(&response_bytes, None, &headers);
            if !messages.is_empty() || error.is_some() {
                return Ok(WebResponse {
                    messages,
                    headers: HashMap::new(),
                    trailers,
                    error,
                });
            }
        }
    };

    Err(anyhow!(
        "Invalid JSON response: {}",
        String::from_utf8_lossy(&response_bytes)
    ))
}

/// ConnectRPC binary unary: `application/proto`
async fn connect_rpc_unary_proto(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    input_desc: &MessageDescriptor,
    output_desc: &MessageDescriptor,
) -> Result<WebResponse> {
    let request_bytes = serialize_message(&request_body, input_desc)?;

    let (status, response_bytes, headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/proto",
        &request_bytes,
    )
    .await?;

    if !status.is_success() {
        if response_bytes.is_empty() {
            return Err(anyhow!("HTTP {} from server", status));
        }
        if let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes) {
            let code = err_body
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");
            let msg = err_body
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            return Err(anyhow!("gRPC error: code={} message={}", code, msg));
        }
        return Err(anyhow!(
            "HTTP {}: {}",
            status,
            String::from_utf8_lossy(&response_bytes)
        ));
    }

    let msg = DynamicMessage::decode(output_desc.clone(), response_bytes.as_slice())
        .with_context(|| "Failed to decode protobuf response")?;
    let result = dynamic_message_to_json(&msg);

    let trailers = HashMap::new();
    let mut error = None;
    if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
        let msg = headers.get("grpc-message").cloned().unwrap_or_default();
        error = Some(format!("gRPC error: code={} message={}", grpc_status, msg));
    }
    let response_headers: HashMap<String, String> = headers
        .into_iter()
        .filter(|(k, _)| !k.starts_with("grpc-") && *k != "content-type" && *k != "content-length")
        .collect();

    Ok(WebResponse {
        messages: vec![result],
        headers: response_headers,
        trailers,
        error,
    })
}

/// ConnectRPC streaming JSON: `application/connect+json` with Connect envelope framing
async fn connect_rpc_stream_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let body =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;
    let framed = encode_connect_envelope(&body, true);

    let (status, response_bytes, headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/connect+json",
        &framed,
    )
    .await?;

    if !status.is_success() {
        if response_bytes.is_empty() {
            return Err(anyhow!("HTTP {} from server", status));
        }
        if let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes) {
            let code = err_body
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");
            let msg = err_body
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            return Err(anyhow!("gRPC error: code={} message={}", code, msg));
        }
        return Err(anyhow!(
            "HTTP {}: {}",
            status,
            String::from_utf8_lossy(&response_bytes)
        ));
    }

    let (messages, trailers, error) = parse_connect_framed(&response_bytes, None, &headers);
    Ok(WebResponse {
        messages,
        headers: HashMap::new(),
        trailers,
        error,
    })
}

/// ConnectRPC streaming proto: `application/connect+proto` with Connect envelope framing
async fn connect_rpc_stream_proto(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    input_desc: &MessageDescriptor,
    output_desc: &MessageDescriptor,
) -> Result<WebResponse> {
    let request_bytes = serialize_message(&request_body, input_desc)?;

    let framed = encode_connect_envelope(&request_bytes, true);

    let (status, response_bytes, headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/connect+proto",
        &framed,
    )
    .await?;

    if !status.is_success() {
        if response_bytes.is_empty() {
            return Err(anyhow!("HTTP {} from server", status));
        }
        if let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes) {
            let code = err_body
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown");
            let msg = err_body
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            return Err(anyhow!("gRPC error: code={} message={}", code, msg));
        }
        return Err(anyhow!(
            "HTTP {}: {}",
            status,
            String::from_utf8_lossy(&response_bytes)
        ));
    }

    let (messages, trailers, error) =
        parse_connect_framed(&response_bytes, Some(output_desc), &headers);
    Ok(WebResponse {
        messages,
        headers: HashMap::new(),
        trailers,
        error,
    })
}

// ─── gRPC-Web (binary proto or JSON) ────────────────────────

async fn grpc_web(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    _rpc_mode: Option<RpcMode>,
) -> Result<WebResponse> {
    if config.proto_config.is_some() {
        let m = resolve_method(config, service_name, method_name).await?;
        return grpc_web_binary(
            config,
            service_name,
            method_name,
            request_body,
            &m.input_desc,
            &m.output_desc,
        )
        .await;
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

    let (status, response_bytes, headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/grpc-web+json",
        &body,
    )
    .await?;

    if !status.is_success() {
        let response_headers: HashMap<String, String> = headers
            .into_iter()
            .filter(|(k, _)| {
                !k.starts_with("grpc-") && *k != "content-type" && *k != "content-length"
            })
            .collect();
        let error_msg = if response_bytes.is_empty() {
            format!("HTTP {} from server", status)
        } else {
            format!(
                "HTTP {}: {}",
                status,
                String::from_utf8_lossy(&response_bytes)
            )
        };
        return Ok(WebResponse {
            error: Some(error_msg),
            headers: response_headers,
            ..Default::default()
        });
    }

    let (mut messages, trailers, mut error) = parse_grpc_web_framed_json(&response_bytes);
    enrich_grpc_web_error(&mut messages, &mut error);

    let response_headers: HashMap<String, String> = headers
        .into_iter()
        .filter(|(k, _)| !k.starts_with("grpc-") && *k != "content-type" && *k != "content-length")
        .collect();

    Ok(WebResponse {
        messages,
        headers: response_headers,
        trailers,
        error,
    })
}

async fn grpc_web_binary(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    input_desc: &MessageDescriptor,
    output_desc: &MessageDescriptor,
) -> Result<WebResponse> {
    let request_bytes = serialize_message(&request_body, input_desc)?;

    let len = request_bytes.len() as u32;
    let mut body = Vec::with_capacity(request_bytes.len() + 5);
    body.push(0x00);
    body.extend_from_slice(&len.to_be_bytes());
    body.extend_from_slice(&request_bytes);

    let (status, response_bytes, _headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/grpc-web+proto",
        &body,
    )
    .await?;

    if !status.is_success() {
        return if response_bytes.is_empty() {
            Err(anyhow!("HTTP {} from server", status))
        } else {
            Err(anyhow!(
                "HTTP {}: {}",
                status,
                String::from_utf8_lossy(&response_bytes)
            ))
        };
    }

    let (messages, trailers, error) = parse_grpc_web_framed_proto(&response_bytes, output_desc);
    Ok(WebResponse {
        messages,
        headers: HashMap::new(),
        trailers,
        error,
    })
}

/// Encode multiple requests as a ConnectRPC envelope stream.
pub(crate) fn encode_multi_request(requests: &[Value]) -> Vec<u8> {
    let mut buf = Vec::new();
    for req in requests {
        let body = serde_json::to_vec(req).unwrap_or_default();
        let framed = encode_connect_envelope(&body, false);
        buf.extend_from_slice(&framed);
    }
    let end = encode_connect_envelope(b"", true);
    buf.extend_from_slice(&end);
    buf
}

/// Encode multiple requests as a gRPC-Web frame stream.
/// Each message becomes a data frame (flag 0x00), terminated by a trailers frame
/// (flag 0x80) with grpc-status: 0.
pub(crate) fn encode_multi_request_grpc_web(requests: &[Value]) -> Vec<u8> {
    let mut buf = Vec::new();
    for req in requests {
        let body = serde_json::to_vec(req).unwrap_or_default();
        let len = body.len() as u32;
        buf.push(0x00); // data frame flag
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(&body);
    }
    // No explicit end-of-stream frame — the HTTP connection closing
    // signals end of stream in gRPC-Web.
    buf
}

/// Public wrapper for parse_connect_framed so client.rs can use it.
pub(crate) fn parse_connect_framed_public(
    data: &[u8],
    output_desc: Option<&prost_reflect::MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    parse_connect_framed(data, output_desc, headers)
}

/// Public wrapper for parse_grpc_web_framed_json so client.rs can use it.
pub(crate) fn parse_grpc_web_framed_json_public(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    parse_grpc_web_framed_json(data)
}

/// Extract structured error details from gRPC-Web data frame when trailers indicate an error.
/// If the last data frame contains a google.rpc.Status JSON (has "code" + "message" fields),
/// enrich the error with the structured details.
pub(crate) fn enrich_grpc_web_error(messages: &mut Vec<Value>, error: &mut Option<String>) {
    if error.is_some() && !messages.is_empty() {
        let last_msg = messages.last().unwrap();
        let has_status = last_msg.get("code").is_some() && last_msg.get("message").is_some();
        if has_status {
            let code_val = last_msg.get("code").and_then(|c| c.as_i64()).unwrap_or(2);
            let msg_val = last_msg
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let details = last_msg.get("details").filter(|d| d.is_array());
            let details_str = details.map(|d| d.to_string()).unwrap_or_default();
            *error = Some(format!(
                "gRPC error: code={} message={} details=[{}]",
                code_val, msg_val, details_str
            ));
            messages.pop();
        }
    }
}

// ─── HTTP POST helper ───────────────────────────────────────

pub(crate) async fn send_http_post(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(reqwest::StatusCode, Vec<u8>, ResponseHeaders)> {
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

    let headers = extract_headers(response.headers());
    let status = response.status();
    let response_bytes = response
        .bytes()
        .await
        .with_context(|| "Failed to read response")?;

    Ok((status, response_bytes.to_vec(), headers))
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

fn parse_grpc_web_frame_header(data: &[u8], offset: &mut usize) -> Option<(u8, usize)> {
    if *offset + 5 > data.len() {
        return None;
    }
    let flags = data[*offset];
    let len = u32::from_be_bytes([
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
        data[*offset + 4],
    ]) as usize;
    *offset += 5;
    if *offset + len > data.len() {
        return None;
    }
    *offset += len;
    Some((flags, len))
}

fn percent_decode(s: &str) -> String {
    let mut buf = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(hex_val);
            let lo = chars.next().and_then(hex_val);
            if let (Some(h), Some(l)) = (hi, lo) {
                buf.push((h << 4) | l);
            } else {
                buf.push(b'%');
            }
        } else {
            buf.push(b);
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_grpc_web_trailers(
    payload: &[u8],
    trailers: &mut HashMap<String, String>,
    error: &mut Option<String>,
) {
    let text = String::from_utf8_lossy(payload);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(": ") {
            trailers.insert(k.to_ascii_lowercase(), percent_decode(v));
        }
    }
    if let Some(status) = trailers.get("grpc-status").filter(|s| *s != "0") {
        let msg = trailers.get("grpc-message").cloned().unwrap_or_default();
        *error = Some(format!("gRPC error: code={} message={}", status, msg));
    }
}

fn parse_grpc_web_framed_json(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut offset = 0;

    while let Some((flags, _len)) = parse_grpc_web_frame_header(data, &mut offset) {
        let payload = &data[offset - _len..offset];
        if flags & 0x80 != 0 {
            parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        } else if let Ok(val) = serde_json::from_slice(payload) {
            messages.push(val);
        }
    }

    (messages, trailers, error)
}

fn parse_grpc_web_framed_proto(
    data: &[u8],
    output_desc: &MessageDescriptor,
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut offset = 0;

    while let Some((flags, _len)) = parse_grpc_web_frame_header(data, &mut offset) {
        let payload = &data[offset - _len..offset];
        if flags & 0x80 != 0 {
            parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        } else if let Ok(msg) = DynamicMessage::decode(output_desc.clone(), payload) {
            messages.push(dynamic_message_to_json(&msg));
        }
    }
    if messages.is_empty()
        && trailers.is_empty()
        && !data.is_empty()
        && let Ok(msg) = DynamicMessage::decode(output_desc.clone(), data)
    {
        messages.push(dynamic_message_to_json(&msg));
    }

    (messages, trailers, error)
}

fn dynamic_message_to_json(msg: &DynamicMessage) -> Value {
    let options = SerializeOptions::new().use_proto_field_name(true);
    msg.serialize_with_options(serde_json::value::Serializer, &options)
        .unwrap_or(Value::Null)
}

#[cfg(test)]
fn make_test_descriptor_pool() -> prost_reflect::DescriptorPool {
    use prost_types::{
        DescriptorProto, FileDescriptorProto, FileDescriptorSet, field_descriptor_proto::Type,
    };
    let file = FileDescriptorProto {
        name: Some("test.proto".to_string()),
        package: Some("test".to_string()),
        message_type: vec![
            DescriptorProto {
                name: Some("TestRequest".to_string()),
                field: vec![prost_types::FieldDescriptorProto {
                    name: Some("name".to_string()),
                    number: Some(1),
                    r#type: Some(Type::String.into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            DescriptorProto {
                name: Some("TestResponse".to_string()),
                field: vec![prost_types::FieldDescriptorProto {
                    name: Some("reply".to_string()),
                    number: Some(1),
                    r#type: Some(Type::String.into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let fds = FileDescriptorSet { file: vec![file] };
    prost_reflect::DescriptorPool::from_file_descriptor_set(fds).unwrap()
}

#[cfg(test)]
fn make_test_message(desc: &MessageDescriptor) -> DynamicMessage {
    let val = serde_json::json!({"name": "test-value"});
    let json_str = serde_json::to_string(&val).unwrap();
    let mut de = serde_json::Deserializer::from_str(&json_str);
    DynamicMessage::deserialize(desc.clone(), &mut de).unwrap()
}

// ─── Connect envelope helpers ────────────────────────────────

/// Encode a ConnectRPC envelope frame.
/// Per spec: bit 0 = compressed, bit 1 = end_stream.
/// See: https://connectrpc.com/docs/protocol/#streaming-rpcs
fn encode_connect_envelope(data: &[u8], end_stream: bool) -> Vec<u8> {
    let len = data.len() as u32;
    let mut buf = Vec::with_capacity(data.len() + 5);
    let flags: u8 = if end_stream { 0x02 } else { 0x00 };
    buf.push(flags);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(data);
    buf
}

fn parse_connect_framed(
    data: &[u8],
    output_desc: Option<&MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> (Vec<Value>, HashMap<String, String>, Option<String>) {
    let mut messages = Vec::new();
    let trailers = HashMap::new();
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

        let is_end_stream = flags & 0x02 != 0;
        if is_end_stream && payload.is_empty() {
            // End stream with no data — check headers for error
            if let Some(status) = headers.get("grpc-status").filter(|s| *s != "0") {
                let msg = headers.get("grpc-message").cloned().unwrap_or_default();
                error = Some(format!("gRPC error: code={} message={}", status, msg));
            }
        } else if is_end_stream {
            // End stream with data — could be a ConnectRPC error JSON
            if let Ok(err_body) = serde_json::from_slice::<Value>(payload) {
                let code = err_body
                    .get("code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("unknown");
                let msg = err_body
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                let details = err_body
                    .get("details")
                    .filter(|d| d.is_array())
                    .map(|d| d.to_string())
                    .unwrap_or_default();
                error = Some(format!(
                    "gRPC error: code={} message={} details=[{}]",
                    code, msg, details
                ));
            } else if let Some(desc) = output_desc
                && let Ok(msg) = DynamicMessage::decode(desc.clone(), payload)
            {
                messages.push(dynamic_message_to_json(&msg));
            }
        } else if let Some(desc) = output_desc
            && let Ok(msg) = DynamicMessage::decode(desc.clone(), payload)
        {
            messages.push(dynamic_message_to_json(&msg));
        } else if let Ok(val) = serde_json::from_slice(payload) {
            messages.push(val);
        }
    }

    // If no envelopes found, try raw payload as fallback
    if messages.is_empty() && offset == 5 && !data.is_empty() && data[0] & 0x02 == 0 {
        let payload = &data[5..];
        if let Some(desc) = output_desc {
            if let Ok(msg) = DynamicMessage::decode(desc.clone(), payload) {
                messages.push(dynamic_message_to_json(&msg));
            }
        } else if let Ok(val) = serde_json::from_slice(payload) {
            messages.push(val);
        }
    }

    (messages, trailers, error)
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Connect envelope ─────────────────────────────────────

    #[test]
    fn test_encode_connect_envelope_data() {
        let data = b"hello";
        let framed = encode_connect_envelope(data, false);
        assert_eq!(framed.len(), 10);
        assert_eq!(framed[0], 0x00);
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]);
        assert_eq!(len, 5);
        assert_eq!(&framed[5..], b"hello");
    }

    #[test]
    fn test_encode_connect_envelope_end_stream() {
        let data = b"x";
        let framed = encode_connect_envelope(data, true);
        assert_eq!(framed[0], 0x02);
    }

    #[test]
    fn test_encode_connect_envelope_empty() {
        let framed = encode_connect_envelope(b"", true);
        assert_eq!(framed.len(), 5);
        assert_eq!(framed[0], 0x02);
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]);
        assert_eq!(len, 0);
    }

    // ── gRPC-Web frame header ────────────────────────────────

    #[test]
    fn test_parse_grpc_web_frame_header_basic() {
        let data = vec![0x80, 0x00, 0x00, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let mut offset = 0;
        let result = parse_grpc_web_frame_header(&data, &mut offset);
        assert!(result.is_some());
        let (flags, len) = result.unwrap();
        assert_eq!(flags, 0x80);
        assert_eq!(len, 5);
        assert_eq!(offset, 10);
    }

    #[test]
    fn test_parse_grpc_web_frame_header_too_short() {
        let data = vec![0x00, 0x00, 0x00];
        let mut offset = 0;
        let result = parse_grpc_web_frame_header(&data, &mut offset);
        assert!(result.is_none());
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_parse_grpc_web_frame_header_truncated_payload() {
        let data = vec![0x00, 0x00, 0x00, 0x00, 0x0A, b'h'];
        let mut offset = 0;
        let result = parse_grpc_web_frame_header(&data, &mut offset);
        assert!(result.is_none());
        assert_eq!(offset, 5);
    }

    // ── gRPC-Web framed JSON ─────────────────────────────────

    #[test]
    fn test_parse_grpc_web_framed_json_single() {
        let msg = json!({"key": "value"});
        let body = serde_json::to_vec(&msg).unwrap();
        let len = body.len() as u32;
        let mut data = vec![0x00];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(&body);

        let (messages, trailers, error) = parse_grpc_web_framed_json(&data);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["key"], "value");
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_grpc_web_framed_json_multiple() {
        let msg1 = json!({"seq": 1});
        let msg2 = json!({"seq": 2});
        let mut data = Vec::new();
        for m in [&msg1, &msg2] {
            let body = serde_json::to_vec(m).unwrap();
            let len = body.len() as u32;
            data.push(0x00);
            data.extend_from_slice(&len.to_be_bytes());
            data.extend_from_slice(&body);
        }

        let (messages, _, _) = parse_grpc_web_framed_json(&data);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["seq"], 1);
        assert_eq!(messages[1]["seq"], 2);
    }

    #[test]
    fn test_parse_grpc_web_framed_json_empty() {
        let (messages, trailers, error) = parse_grpc_web_framed_json(b"");
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_grpc_web_framed_json_only_trailers() {
        let trailer_data = b"grpc-status: 5\ngrpc-message: not found";
        let len = trailer_data.len() as u32;
        let mut data = vec![0x80];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(trailer_data);

        let (messages, trailers, error) = parse_grpc_web_framed_json(&data);
        assert!(messages.is_empty());
        assert_eq!(trailers.get("grpc-status").unwrap(), "5");
        assert_eq!(trailers.get("grpc-message").unwrap(), "not found");
        assert!(error.is_some());
        assert!(error.unwrap().contains("code=5"));
    }

    #[test]
    fn test_parse_grpc_web_framed_json_data_then_trailers() {
        let msg = json!({"done": true});
        let body = serde_json::to_vec(&msg).unwrap();
        let mut data = Vec::new();
        data.push(0x00);
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&body);
        let trailer = b"grpc-status: 0";
        data.push(0x80);
        data.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
        data.extend_from_slice(trailer);

        let (messages, trailers, error) = parse_grpc_web_framed_json(&data);
        assert_eq!(messages.len(), 1);
        assert!(messages[0]["done"].as_bool().unwrap());
        assert_eq!(trailers.get("grpc-status").unwrap(), "0");
        assert!(error.is_none());
    }

    // ── gRPC-Web trailer parsing ─────────────────────────────

    #[test]
    fn test_parse_grpc_web_trailers_case_folding() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"Grpc-Status: 3\nGRPC-MESSAGE: bad";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("grpc-status").unwrap(), "3");
        assert_eq!(trailers.get("grpc-message").unwrap(), "bad");
        assert!(error.is_some());
    }

    #[test]
    fn test_parse_grpc_web_trailers_mixed_case() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"Grpc-Status: 4\nGrpc-Message: deadline exceeded";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("grpc-status").unwrap(), "4");
        assert!(error.is_some());
        assert!(error.unwrap().contains("code=4"));
    }

    #[test]
    fn test_parse_grpc_web_trailers_success() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"grpc-status: 0";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_grpc_web_trailers_custom_metadata() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"custom-key: custom-value\nx-trace-id: abc123";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("custom-key").unwrap(), "custom-value");
        assert_eq!(trailers.get("x-trace-id").unwrap(), "abc123");
        assert!(error.is_none());
    }

    // ── gRPC-Web framed proto ────────────────────────────────

    #[test]
    fn test_parse_grpc_web_framed_proto_data_frame() {
        let pool = make_test_descriptor_pool();
        let output = pool.get_message_by_name("test.TestResponse").unwrap();
        let msg = make_test_message(&pool.get_message_by_name("test.TestRequest").unwrap());

        let body = msg.encode_to_vec();
        let len = body.len() as u32;
        let mut data = vec![0x00];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(&body);

        let (_messages, trailers, error) = parse_grpc_web_framed_proto(&data, &output);
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_grpc_web_framed_proto_empty() {
        let pool = make_test_descriptor_pool();
        let output = pool.get_message_by_name("test.TestResponse").unwrap();
        let (messages, trailers, error) = parse_grpc_web_framed_proto(b"", &output);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_grpc_web_framed_proto_trailers() {
        let pool = make_test_descriptor_pool();
        let output = pool.get_message_by_name("test.TestResponse").unwrap();
        let trailer_data = b"grpc-status: 3\ngrpc-message: bad";
        let len = trailer_data.len() as u32;
        let mut data = vec![0x80];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(trailer_data);

        let (messages, trailers, error) = parse_grpc_web_framed_proto(&data, &output);
        assert!(messages.is_empty());
        assert_eq!(trailers.get("grpc-status").unwrap(), "3");
        assert!(error.is_some());
    }

    // ── Connect framing ──────────────────────────────────────

    #[test]
    fn test_parse_connect_framed_data_json() {
        let msg = json!({"key": "val"});
        let body = serde_json::to_vec(&msg).unwrap();
        let framed = encode_connect_envelope(&body, false);
        let headers = HashMap::new();

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["key"], "val");
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_connect_framed_end_stream_error_json() {
        let err = json!({"code": "unavailable", "message": "service down"});
        let body = serde_json::to_vec(&err).unwrap();
        let framed = encode_connect_envelope(&body, true);
        let headers = HashMap::new();

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_some());
        let e = error.unwrap();
        assert!(e.contains("code=unavailable"));
        assert!(e.contains("message=service down"));
    }

    #[test]
    fn test_parse_connect_framed_end_stream_empty_with_header_error() {
        let framed = encode_connect_envelope(b"", true);
        let mut headers = HashMap::new();
        headers.insert("grpc-status".to_string(), "5".to_string());
        headers.insert("grpc-message".to_string(), "not found".to_string());

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_some());
        assert!(error.unwrap().contains("code=5"));
    }

    #[test]
    fn test_parse_connect_framed_end_stream_empty_no_error() {
        let framed = encode_connect_envelope(b"", true);
        let headers = HashMap::new();

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn test_parse_connect_framed_multiple_data_frames() {
        let headers = HashMap::new();
        let mut all_framed = Vec::new();
        for i in 0..3 {
            let msg = json!({"seq": i});
            let body = serde_json::to_vec(&msg).unwrap();
            all_framed.extend_from_slice(&encode_connect_envelope(&body, false));
        }
        all_framed.extend_from_slice(&encode_connect_envelope(b"", true));

        let (messages, trailers, error) = parse_connect_framed(&all_framed, None, &headers);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["seq"], 0);
        assert_eq!(messages[2]["seq"], 2);
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    // ── dynamic_message_to_json ──────────────────────────────

    #[test]
    fn test_dynamic_message_to_json_empty_message() {
        let pool = make_test_descriptor_pool();
        let desc = pool.get_message_by_name("test.TestResponse").unwrap();
        let val = dynamic_message_to_json(&DynamicMessage::new(desc));
        // Empty message serializes to {} not null
        assert_eq!(val, json!({}));
    }

    // ── extract_headers ──────────────────────────────────────

    #[test]
    fn test_extract_headers_normalizes_case() {
        use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
        let mut hm = HeaderMap::new();
        hm.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        hm.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        hm.insert(
            HeaderName::from_static("x-custom"),
            HeaderValue::from_static("val"),
        );

        let result = extract_headers(&hm);
        assert_eq!(result.get("content-type").unwrap(), "application/json");
        assert_eq!(result.get("grpc-status").unwrap(), "0");
        assert_eq!(result.get("x-custom").unwrap(), "val");
    }

    // ── Serialize roundtrip ──────────────────────────────────

    #[test]
    fn test_serialize_message_produces_valid_proto() {
        let pool = make_test_descriptor_pool();
        let input = pool.get_message_by_name("test.TestRequest").unwrap();
        let val = json!({"name": "hello"});
        let bytes = serialize_message(&val, &input).unwrap();
        assert!(!bytes.is_empty());

        let decoded = DynamicMessage::decode(input.clone(), &bytes[..]).unwrap();
        let json = dynamic_message_to_json(&decoded);
        assert_eq!(json["name"], "hello");
    }

    #[test]
    fn test_serialize_message_with_known_field() {
        let pool = make_test_descriptor_pool();
        let input = pool.get_message_by_name("test.TestRequest").unwrap();
        let val = json!({"name": "hello"});
        let bytes = serialize_message(&val, &input).unwrap();
        assert!(!bytes.is_empty());

        let decoded = DynamicMessage::decode(input.clone(), &bytes[..]).unwrap();
        let json = dynamic_message_to_json(&decoded);
        assert_eq!(json["name"], "hello");
    }
}
