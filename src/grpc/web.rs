#![allow(clippy::unwrap_used, clippy::expect_used)]
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

fn public_response_headers(headers: ResponseHeaders) -> ResponseHeaders {
    headers
        .into_iter()
        .filter(|(k, _)| !k.starts_with("grpc-") && k != "content-type" && k != "content-length")
        .collect()
}

fn split_connect_trailers(headers: ResponseHeaders) -> (ResponseHeaders, ResponseHeaders) {
    let mut leading = ResponseHeaders::new();
    let mut trailers = ResponseHeaders::new();

    for (key, value) in headers {
        match key.strip_prefix("trailer-") {
            Some(stripped) if !stripped.is_empty() => {
                trailers.insert(stripped.to_string(), value);
            }
            _ => {
                leading.insert(key, value);
            }
        }
    }

    (public_response_headers(leading), trailers)
}

fn is_grpc_web_text(headers: &ResponseHeaders) -> bool {
    headers
        .get("content-type")
        .is_some_and(|c| c.contains("grpc-web-text"))
}

fn decode_grpc_web_body(body: Vec<u8>, headers: &ResponseHeaders) -> Vec<u8> {
    if is_grpc_web_text(headers) {
        base64_decode(&body).unwrap_or(body)
    } else {
        body
    }
}

fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut quad = [0u8; 4];
    let mut n = 0;
    let mut pads = 0;
    for &b in input {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            quad[n] = 0;
            pads += 1;
            n += 1;
        } else {
            if pads != 0 {
                return None;
            }
            quad[n] = val(b)?;
            n += 1;
        }
        if n == 4 {
            out.push((quad[0] << 2) | (quad[1] >> 4));
            if pads < 2 {
                out.push((quad[1] << 4) | (quad[2] >> 2));
            }
            if pads < 1 {
                out.push((quad[2] << 6) | quad[3]);
            }
            n = 0;
            pads = 0;
        }
    }
    if n != 0 {
        return None;
    }
    Some(out)
}

fn base64_encode(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(A[b0 >> 2] as char);
        out.push(A[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 {
            A[((b1 & 0x0f) << 2) | (b2 >> 6)] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[b2 & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode_lenient(input: &[u8]) -> Option<Vec<u8>> {
    let mut cleaned: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    match cleaned.len() % 4 {
        0 => {}
        1 => return None,
        r => cleaned.resize(cleaned.len() + (4 - r), b'='),
    }
    base64_decode(&cleaned)
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .with_context(|| "Failed to gzip request message")?;
    encoder
        .finish()
        .with_context(|| "Failed to finish gzip stream")
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .with_context(|| "Failed to gunzip response message")?;
    Ok(out)
}

use crate::grpc::{
    CompressionMode, GrpcClientConfig, GrpcError, RpcMode, TlsConfig, TransportResult, WireProtocol,
};
use futures::{Stream, StreamExt};
use std::sync::{LazyLock, Mutex};

#[derive(Default)]
struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    fn new() -> Self {
        Self::default()
    }

    fn extend(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    fn next_frame(&mut self) -> Option<(u8, Vec<u8>)> {
        if self.buf.len() < 5 {
            return None;
        }
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if self.buf.len() < 5 + len {
            return None;
        }
        let flags = self.buf[0];
        let payload = self.buf[5..5 + len].to_vec();
        self.buf.drain(..5 + len);
        Some((flags, payload))
    }

    fn remaining(&self) -> &[u8] {
        &self.buf
    }
}

fn compression_header(
    content_type: &str,
    config: &GrpcClientConfig,
) -> Option<(&'static str, &'static str)> {
    if config.compression != CompressionMode::Gzip {
        return None;
    }
    if content_type.contains("grpc-web") {
        Some(("grpc-encoding", "gzip"))
    } else if content_type.starts_with("application/connect+") {
        Some(("connect-content-encoding", "gzip"))
    } else if content_type == "application/proto" || content_type == "application/json" {
        Some(("content-encoding", "gzip"))
    } else {
        None
    }
}

fn maybe_gzip_request(body: Vec<u8>, config: &GrpcClientConfig) -> Result<Vec<u8>> {
    if config.compression == CompressionMode::Gzip {
        gzip_compress(&body)
    } else {
        Ok(body)
    }
}

fn frames_to_body(frames: Vec<Vec<u8>>) -> reqwest::Body {
    reqwest::Body::wrap_stream(futures::stream::iter(
        frames.into_iter().map(Ok::<Vec<u8>, std::io::Error>),
    ))
}

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
    pub message_offsets_ms: Vec<u64>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub error: Option<GrpcError>,
}

fn grpc_code_from_token(token: &str) -> u32 {
    token.parse::<u32>().unwrap_or(match token {
        "cancelled" => 1,
        "unknown" => 2,
        "invalid_argument" => 3,
        "deadline_exceeded" => 4,
        "not_found" => 5,
        "already_exists" => 6,
        "permission_denied" => 7,
        "resource_exhausted" => 8,
        "failed_precondition" => 9,
        "aborted" => 10,
        "out_of_range" => 11,
        "unimplemented" => 12,
        "internal" => 13,
        "unavailable" => 14,
        "data_loss" => 15,
        "unauthenticated" => 16,
        _ => 2,
    })
}

fn connect_error_from_json(err: &Value) -> GrpcError {
    let code = err
        .get("code")
        .and_then(|c| c.as_str())
        .map(grpc_code_from_token)
        .unwrap_or(2);
    let message = err
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let details = err
        .get("details")
        .filter(|d| d.is_array())
        .map(|d| d.to_string().into_bytes())
        .unwrap_or_default();
    GrpcError::with_details(code, message, details)
}

fn trailer_status_error(code_token: &str, message: String) -> GrpcError {
    GrpcError::new(grpc_code_from_token(code_token), message)
}

impl From<WebResponse> for TransportResult {
    fn from(r: WebResponse) -> Self {
        TransportResult {
            messages: r.messages,
            message_offsets_ms: r.message_offsets_ms,
            headers: r.headers,
            trailers: r.trailers,
            error: r.error,
        }
    }
}

impl WebResponse {
    fn http_error(status: reqwest::StatusCode, body: &[u8], headers: ResponseHeaders) -> Self {
        let message = if body.is_empty() {
            format!("HTTP {} from server", status)
        } else {
            format!("HTTP {}: {}", status, String::from_utf8_lossy(body))
        };
        WebResponse {
            error: Some(GrpcError::new(2, message)),
            headers: public_response_headers(headers),
            ..Default::default()
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
        RpcMode::Unary => {
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
        RpcMode::ClientStream | RpcMode::ServerStream | RpcMode::Bidi => {
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

async fn connect_rpc_unary_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let body =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;
    let body = maybe_gzip_request(body, config)?;

    let (status, response_bytes, headers) =
        send_http_post(config, service_name, method_name, "application/json", &body).await?;

    if !status.is_success() {
        if !response_bytes.is_empty()
            && let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes)
        {
            return Ok(WebResponse {
                error: Some(connect_error_from_json(&err_body)),
                headers: public_response_headers(headers),
                ..Default::default()
            });
        }
        return Ok(WebResponse::http_error(status, &response_bytes, headers));
    }

    match serde_json::from_slice::<Value>(&response_bytes) {
        Ok(v) => {
            let mut error = None;
            if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
                let msg = headers.get("grpc-message").cloned().unwrap_or_default();
                error = Some(trailer_status_error(grpc_status, msg));
            }
            let (response_headers, trailers) = split_connect_trailers(headers);
            return Ok(WebResponse {
                messages: vec![v],
                message_offsets_ms: vec![0],
                headers: response_headers,
                trailers,
                error,
            });
        }
        Err(_) => {
            let (messages, trailers, error) = parse_connect_framed(&response_bytes, None, &headers);
            if !messages.is_empty() || error.is_some() {
                return Ok(WebResponse {
                    messages,
                    message_offsets_ms: vec![],
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

async fn connect_rpc_unary_proto(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    input_desc: &MessageDescriptor,
    output_desc: &MessageDescriptor,
) -> Result<WebResponse> {
    let request_bytes = serialize_message(&request_body, input_desc)?;
    let request_bytes = maybe_gzip_request(request_bytes, config)?;

    let (status, response_bytes, headers) = send_http_post(
        config,
        service_name,
        method_name,
        "application/proto",
        &request_bytes,
    )
    .await?;

    if !status.is_success() {
        if !response_bytes.is_empty()
            && let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes)
        {
            return Ok(WebResponse {
                error: Some(connect_error_from_json(&err_body)),
                headers: public_response_headers(headers),
                ..Default::default()
            });
        }
        return Ok(WebResponse::http_error(status, &response_bytes, headers));
    }

    let msg = DynamicMessage::decode(output_desc.clone(), response_bytes.as_slice())
        .with_context(|| "Failed to decode protobuf response")?;
    let result = dynamic_message_to_json(&msg);

    let mut error = None;
    if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
        let msg = headers.get("grpc-message").cloned().unwrap_or_default();
        error = Some(trailer_status_error(grpc_status, msg));
    }
    let (response_headers, trailers) = split_connect_trailers(headers);

    Ok(WebResponse {
        messages: vec![result],
        message_offsets_ms: vec![0],
        headers: response_headers,
        trailers,
        error,
    })
}

async fn connect_rpc_stream_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let body =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;
    let compress = config.compression == CompressionMode::Gzip;
    let framed = encode_connect_envelope_compressed(&body, false, compress)?;

    let (status, headers, body_stream) = send_http(
        config,
        service_name,
        method_name,
        "application/connect+json",
        frames_to_body(vec![framed]),
    )
    .await?;
    let mut body_stream = Box::pin(body_stream);

    if !status.is_success() {
        let response_bytes = collect_stream(&mut body_stream).await?;
        if !response_bytes.is_empty()
            && let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes)
        {
            return Ok(WebResponse {
                error: Some(connect_error_from_json(&err_body)),
                headers: public_response_headers(headers),
                ..Default::default()
            });
        }
        return Ok(WebResponse::http_error(status, &response_bytes, headers));
    }

    let (messages, message_offsets_ms, trailers, error) =
        parse_connect_stream(&mut body_stream, None, &headers).await?;
    Ok(WebResponse {
        messages,
        message_offsets_ms,
        headers: public_response_headers(headers),
        trailers,
        error,
    })
}

async fn connect_rpc_stream_proto(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
    input_desc: &MessageDescriptor,
    output_desc: &MessageDescriptor,
) -> Result<WebResponse> {
    let request_bytes = serialize_message(&request_body, input_desc)?;
    let compress = config.compression == CompressionMode::Gzip;
    let framed = encode_connect_envelope_compressed(&request_bytes, false, compress)?;

    let (status, headers, body_stream) = send_http(
        config,
        service_name,
        method_name,
        "application/connect+proto",
        frames_to_body(vec![framed]),
    )
    .await?;
    let mut body_stream = Box::pin(body_stream);

    if !status.is_success() {
        let response_bytes = collect_stream(&mut body_stream).await?;
        if !response_bytes.is_empty()
            && let Ok(err_body) = serde_json::from_slice::<Value>(&response_bytes)
        {
            return Ok(WebResponse {
                error: Some(connect_error_from_json(&err_body)),
                headers: public_response_headers(headers),
                ..Default::default()
            });
        }
        return Ok(WebResponse::http_error(status, &response_bytes, headers));
    }

    let (messages, message_offsets_ms, trailers, error) =
        parse_connect_stream(&mut body_stream, Some(output_desc), &headers).await?;
    Ok(WebResponse {
        messages,
        message_offsets_ms,
        headers: public_response_headers(headers),
        trailers,
        error,
    })
}

const GRPC_WEB_TEXT_FLAG: &str = "grpc-web-text";

fn grpc_web_text_enabled(config: &GrpcClientConfig) -> bool {
    config.metadata.as_ref().is_some_and(|m| {
        m.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case(GRPC_WEB_TEXT_FLAG)
                && (v.trim().eq_ignore_ascii_case("true") || v.trim() == "1")
        })
    })
}

fn frame_grpc_web_request(
    payload: Vec<u8>,
    base_content_type: &str,
    config: &GrpcClientConfig,
) -> Result<(Vec<u8>, String)> {
    let (flag, payload) = if config.compression == CompressionMode::Gzip {
        (0x01u8, gzip_compress(&payload)?)
    } else {
        (0x00u8, payload)
    };

    let len = payload.len() as u32;
    let mut framed = Vec::with_capacity(payload.len() + 5);
    framed.push(flag);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(&payload);

    if grpc_web_text_enabled(config) {
        let body = base64_encode(&framed).into_bytes();
        let content_type = base_content_type.replace("grpc-web", "grpc-web-text");
        Ok((body, content_type))
    } else {
        Ok((framed, base_content_type.to_string()))
    }
}

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

async fn grpc_web_json(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    request_body: Value,
) -> Result<WebResponse> {
    let json_bytes =
        serde_json::to_vec(&request_body).with_context(|| "Failed to serialize request body")?;

    let (body, content_type) =
        frame_grpc_web_request(json_bytes, "application/grpc-web+json", config)?;

    let (status, headers, body_stream) = send_http(
        config,
        service_name,
        method_name,
        &content_type,
        reqwest::Body::from(body),
    )
    .await?;
    let mut body_stream = Box::pin(body_stream);

    if !status.is_success() {
        let response_bytes = collect_stream(&mut body_stream).await?;
        return Ok(WebResponse::http_error(status, &response_bytes, headers));
    }

    let (mut messages, mut message_offsets_ms, mut trailers, mut error) =
        parse_grpc_web_response(&mut body_stream, None, &headers).await?;
    apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);
    enrich_grpc_web_error(&mut messages, &mut error);
    message_offsets_ms.truncate(messages.len());

    let response_headers = public_response_headers(headers);

    Ok(WebResponse {
        messages,
        message_offsets_ms,
        headers: response_headers,
        trailers,
        error,
    })
}

async fn parse_grpc_web_response<S>(
    body_stream: &mut S,
    output_desc: Option<&MessageDescriptor>,
    headers: &ResponseHeaders,
) -> Result<(
    Vec<Value>,
    Vec<u64>,
    HashMap<String, String>,
    Option<GrpcError>,
)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    if is_grpc_web_text(headers) {
        let raw = collect_stream(body_stream).await?;
        let framed = decode_grpc_web_body(raw, headers);
        let (messages, trailers, error) = match output_desc {
            Some(desc) => parse_grpc_web_framed_proto(&framed, desc),
            None => parse_grpc_web_framed_json(&framed),
        };
        Ok((messages, vec![], trailers, error))
    } else {
        parse_grpc_web_stream(body_stream, output_desc).await
    }
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

    let (body, content_type) =
        frame_grpc_web_request(request_bytes, "application/grpc-web+proto", config)?;

    let (status, headers, body_stream) = send_http(
        config,
        service_name,
        method_name,
        &content_type,
        reqwest::Body::from(body),
    )
    .await?;
    let mut body_stream = Box::pin(body_stream);

    if !status.is_success() {
        let response_bytes = collect_stream(&mut body_stream).await?;
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

    let (messages, message_offsets_ms, mut trailers, mut error) =
        parse_grpc_web_response(&mut body_stream, Some(output_desc), &headers).await?;
    apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);
    Ok(WebResponse {
        messages,
        message_offsets_ms,
        headers: public_response_headers(headers),
        trailers,
        error,
    })
}

fn frame_messages_connect(requests: &[Value]) -> Vec<Vec<u8>> {
    requests
        .iter()
        .map(|req| {
            let body = serde_json::to_vec(req).unwrap_or_default();
            encode_connect_envelope(&body, false)
        })
        .collect()
}

fn frame_messages_grpc_web(requests: &[Value]) -> Vec<Vec<u8>> {
    requests
        .iter()
        .map(|req| {
            let body = serde_json::to_vec(req).unwrap_or_default();
            let mut frame = Vec::with_capacity(body.len() + 5);
            frame.push(0x00);
            frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frame.extend_from_slice(&body);
            frame
        })
        .collect()
}

pub(crate) fn encode_connect_frame(data: &[u8]) -> Vec<u8> {
    encode_connect_envelope(data, false)
}

#[cfg(test)]
pub(crate) fn encode_connect_frame_end() -> Vec<u8> {
    encode_connect_envelope(b"", true)
}

pub(crate) fn encode_grpc_web_frame(data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(data.len() + 5);
    frame.push(0x00);
    frame.extend_from_slice(&(data.len() as u32).to_be_bytes());
    frame.extend_from_slice(data);
    frame
}

pub(crate) fn data_frame_payloads(data: &[u8]) -> Vec<Vec<u8>> {
    let mut decoder = FrameDecoder::new();
    decoder.extend(data);

    let mut payloads = Vec::new();
    while let Some((flags, payload)) = decoder.next_frame() {
        if flags == 0x00 {
            payloads.push(payload);
        }
    }

    payloads
}

pub(crate) fn encode_multi_request(requests: &[Value]) -> Vec<u8> {
    frame_messages_connect(requests).concat()
}

pub(crate) fn encode_multi_request_grpc_web(requests: &[Value]) -> Vec<u8> {
    frame_messages_grpc_web(requests).concat()
}

pub(crate) fn parse_connect_framed_public(
    data: &[u8],
    output_desc: Option<&prost_reflect::MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    parse_connect_framed(data, output_desc, headers)
}

pub(crate) fn parse_grpc_web_framed_json_public(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    parse_grpc_web_framed_json(data)
}

pub(crate) fn enrich_grpc_web_error(messages: &mut Vec<Value>, error: &mut Option<GrpcError>) {
    if error.as_ref().is_some_and(|e| !e.details.is_empty()) {
        return;
    }
    if error.is_some() && !messages.is_empty() {
        let last_msg = messages.last().unwrap();
        let has_status = last_msg.get("code").is_some() && last_msg.get("message").is_some();
        if has_status {
            let code_val = last_msg.get("code").and_then(|c| c.as_i64()).unwrap_or(2) as u32;
            let msg_val = last_msg
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let details = last_msg
                .get("details")
                .filter(|d| d.is_array())
                .map(|d| d.to_string().into_bytes())
                .unwrap_or_default();
            *error = Some(GrpcError::with_details(code_val, msg_val, details));
            messages.pop();
        }
    }
}

const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 5;

fn effective_request_timeout_secs(configured: u64) -> u64 {
    if configured == 0 {
        DEFAULT_HTTP_TIMEOUT_SECS
    } else {
        configured
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpClientCacheKey {
    timeout_seconds: u64,
    tls_config: Option<TlsConfig>,
    connection_id: u64,
}

fn http_client_cache_key(config: &GrpcClientConfig) -> HttpClientCacheKey {
    HttpClientCacheKey {
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        connection_id: config.connection_id,
    }
}

const HTTP_CLIENT_CACHE_MAX_ENTRIES: usize = 512;

struct BoundedHttpClientCache {
    map: HashMap<HttpClientCacheKey, reqwest::Client>,
    order: std::collections::VecDeque<HttpClientCacheKey>,
}

impl BoundedHttpClientCache {
    fn get(&self, key: &HttpClientCacheKey) -> Option<&reqwest::Client> {
        self.map.get(key)
    }

    fn insert(&mut self, key: HttpClientCacheKey, client: reqwest::Client) {
        if self.map.insert(key.clone(), client).is_none() {
            self.order.push_back(key);
        }
        while self.map.len() > HTTP_CLIENT_CACHE_MAX_ENTRIES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
    }
}

static HTTP_CLIENT_CACHE: LazyLock<Mutex<BoundedHttpClientCache>> = LazyLock::new(|| {
    Mutex::new(BoundedHttpClientCache {
        map: HashMap::new(),
        order: std::collections::VecDeque::new(),
    })
});

pub(crate) fn cached_http_client(config: &GrpcClientConfig) -> Result<reqwest::Client> {
    let key = http_client_cache_key(config);
    {
        let cache = HTTP_CLIENT_CACHE.lock().unwrap();
        if let Some(client) = cache.get(&key) {
            return Ok(client.clone());
        }
    }
    let client = build_http_client(config)?;
    let mut cache = HTTP_CLIENT_CACHE.lock().unwrap();
    cache.insert(key, client.clone());
    Ok(client)
}

pub(crate) fn build_http_client(config: &GrpcClientConfig) -> Result<reqwest::Client> {
    let user_agent = format!("grpctestify/{}", env!("CARGO_PKG_VERSION"));

    let mut req_builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            effective_request_timeout_secs(config.timeout_seconds),
        ))
        .connect_timeout(std::time::Duration::from_secs(5))
        .user_agent(&user_agent);

    if let Some(ref tls) = config.tls_config {
        if tls.insecure_skip_verify {
            req_builder = req_builder.danger_accept_invalid_certs(true);
        }

        if let Some(ref ca_path) = tls.ca_cert_path {
            let pem = std::fs::read(ca_path)
                .with_context(|| format!("Failed to read CA certificate '{}'", ca_path))?;
            let cert = reqwest::Certificate::from_pem(&pem)
                .with_context(|| format!("Invalid CA certificate '{}'", ca_path))?;
            req_builder = req_builder.add_root_certificate(cert);
        }

        match (&tls.client_cert_path, &tls.client_key_path) {
            (Some(cert_path), Some(key_path)) => {
                let mut pem = std::fs::read(cert_path).with_context(|| {
                    format!("Failed to read client certificate '{}'", cert_path)
                })?;
                if !pem.ends_with(b"\n") {
                    pem.push(b'\n');
                }
                pem.extend(
                    std::fs::read(key_path)
                        .with_context(|| format!("Failed to read client key '{}'", key_path))?,
                );
                let identity = reqwest::Identity::from_pem(&pem).with_context(|| {
                    format!(
                        "Invalid client identity (cert '{}' + key '{}')",
                        cert_path, key_path
                    )
                })?;
                req_builder = req_builder.identity(identity);
            }
            (None, None) => {}
            _ => {
                return Err(anyhow!(
                    "Both client_cert_path and client_key_path must be set for mTLS (got only one)"
                ));
            }
        }
    }

    req_builder.build().with_context(|| {
        if config.tls_config.is_some() {
            "Failed to build HTTP client (TLS configuration invalid — check ca_cert/client_cert/client_key files)"
        } else {
            "Failed to build HTTP client"
        }
    })
}

fn request_url(config: &GrpcClientConfig, service_name: &str, method_name: &str) -> String {
    let path = format!("/{}/{}", service_name, method_name);
    let scheme = if config.tls_config.is_some() {
        "https"
    } else {
        "http"
    };
    if config.address.starts_with("http://") || config.address.starts_with("https://") {
        format!("{}{}", config.address, path)
    } else {
        format!("{}://{}{}", scheme, config.address, path)
    }
}

fn build_post_request(
    config: &GrpcClientConfig,
    url: &str,
    content_type: &str,
) -> Result<reqwest::RequestBuilder> {
    let http_client = cached_http_client(config)?;
    let mut http_req = http_client.post(url).header("Content-Type", content_type);

    if let Some((name, value)) = compression_header(content_type, config) {
        http_req = http_req.header(name, value);
    }

    if let Some(ref metadata) = config.metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") || k.eq_ignore_ascii_case(GRPC_WEB_TEXT_FLAG) {
                continue;
            }
            http_req = http_req.header(k.as_str(), v.as_str());
        }
    }
    Ok(http_req)
}

pub(crate) async fn send_http_post(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(reqwest::StatusCode, Vec<u8>, ResponseHeaders)> {
    let url = request_url(config, service_name, method_name);
    let response = build_post_request(config, &url, content_type)?
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

async fn send_http(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
    content_type: &str,
    body: reqwest::Body,
) -> Result<(
    reqwest::StatusCode,
    ResponseHeaders,
    impl Stream<Item = Result<Vec<u8>>>,
)> {
    let url = request_url(config, service_name, method_name);
    let response = build_post_request(config, &url, content_type)?
        .body(body)
        .send()
        .await
        .with_context(|| format!("Request to {} failed", url))?;

    let headers = extract_headers(response.headers());
    let status = response.status();
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map(|b| b.to_vec()).map_err(anyhow::Error::from));
    Ok((status, headers, stream))
}

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

async fn collect_stream<S>(mut chunks: S) -> Result<Vec<u8>>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut buf = Vec::new();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        if buf.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(anyhow!(
                "the response is larger than {} MiB, which is more than the client keeps in memory",
                MAX_RESPONSE_BYTES / (1024 * 1024)
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
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
    error: &mut Option<GrpcError>,
) {
    let text = String::from_utf8_lossy(payload);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            trailers.insert(k.trim().to_ascii_lowercase(), percent_decode(v.trim()));
        }
    }
    if let Some(status) = trailers.get("grpc-status").filter(|s| *s != "0") {
        let msg = trailers.get("grpc-message").cloned().unwrap_or_default();
        let mut err = trailer_status_error(status, msg);
        if let Some(details_b64) = trailers.get("grpc-status-details-bin")
            && let Some(bytes) = base64_decode_lenient(details_b64.as_bytes())
        {
            err.details = bytes;
        }
        *error = Some(err);
    }
}

fn apply_grpc_web_header_trailers(
    headers: &ResponseHeaders,
    trailers: &mut HashMap<String, String>,
    error: &mut Option<GrpcError>,
) {
    if trailers.contains_key("grpc-status") {
        return;
    }
    let Some(status) = headers.get("grpc-status") else {
        return;
    };
    trailers.insert("grpc-status".to_string(), status.clone());
    if let Some(msg) = headers.get("grpc-message") {
        trailers.insert("grpc-message".to_string(), percent_decode(msg));
    }
    if let Some(details_b64) = headers.get("grpc-status-details-bin") {
        trailers.insert("grpc-status-details-bin".to_string(), details_b64.clone());
    }
    if status != "0" {
        let msg = headers
            .get("grpc-message")
            .map(|m| percent_decode(m))
            .unwrap_or_default();
        let mut err = trailer_status_error(status, msg);
        if let Some(details_b64) = headers.get("grpc-status-details-bin")
            && let Some(bytes) = base64_decode_lenient(details_b64.as_bytes())
        {
            err.details = bytes;
        }
        *error = Some(err);
    }
}

fn grpc_web_frame_data(flags: u8, raw: &[u8]) -> Option<Vec<u8>> {
    if flags & 0x80 != 0 {
        return None;
    }
    if flags & 0x01 != 0 {
        gzip_decompress(raw).ok()
    } else {
        Some(raw.to_vec())
    }
}

fn push_grpc_web_frame_json(
    flags: u8,
    raw: &[u8],
    messages: &mut Vec<Value>,
    trailers: &mut HashMap<String, String>,
    error: &mut Option<GrpcError>,
) {
    if flags & 0x80 != 0 {
        parse_grpc_web_trailers(raw, trailers, error);
    } else if let Some(payload) = grpc_web_frame_data(flags, raw)
        && let Ok(val) = serde_json::from_slice(&payload)
    {
        messages.push(val);
    }
}

fn push_grpc_web_frame_proto(
    flags: u8,
    raw: &[u8],
    output_desc: &MessageDescriptor,
    messages: &mut Vec<Value>,
    trailers: &mut HashMap<String, String>,
    error: &mut Option<GrpcError>,
) {
    if flags & 0x80 != 0 {
        parse_grpc_web_trailers(raw, trailers, error);
    } else if let Some(payload) = grpc_web_frame_data(flags, raw)
        && let Ok(msg) = DynamicMessage::decode(output_desc.clone(), payload.as_slice())
    {
        messages.push(dynamic_message_to_json(&msg));
    }
}

fn parse_grpc_web_framed_json(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut offset = 0;

    while let Some((_flags, _len)) = parse_grpc_web_frame_header(data, &mut offset) {
        let raw = &data[offset - _len..offset];
        push_grpc_web_frame_json(_flags, raw, &mut messages, &mut trailers, &mut error);
    }

    (messages, trailers, error)
}

fn parse_grpc_web_framed_proto(
    data: &[u8],
    output_desc: &MessageDescriptor,
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut offset = 0;

    while let Some((_flags, _len)) = parse_grpc_web_frame_header(data, &mut offset) {
        let raw = &data[offset - _len..offset];
        push_grpc_web_frame_proto(
            _flags,
            raw,
            output_desc,
            &mut messages,
            &mut trailers,
            &mut error,
        );
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

async fn parse_grpc_web_stream<S>(
    mut chunks: S,
    output_desc: Option<&MessageDescriptor>,
) -> Result<(
    Vec<Value>,
    Vec<u64>,
    HashMap<String, String>,
    Option<GrpcError>,
)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut decoder = FrameDecoder::new();
    let mut messages = Vec::new();
    let mut offsets_ms = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let started = std::time::Instant::now();

    while let Some(chunk) = chunks.next().await {
        decoder.extend(&chunk?);
        while let Some((flags, payload)) = decoder.next_frame() {
            match output_desc {
                Some(desc) => push_grpc_web_frame_proto(
                    flags,
                    &payload,
                    desc,
                    &mut messages,
                    &mut trailers,
                    &mut error,
                ),
                None => push_grpc_web_frame_json(
                    flags,
                    &payload,
                    &mut messages,
                    &mut trailers,
                    &mut error,
                ),
            }
            let arrived = started.elapsed().as_millis() as u64;
            while offsets_ms.len() < messages.len() {
                offsets_ms.push(arrived);
            }
        }
    }

    if let Some(desc) = output_desc
        && messages.is_empty()
        && trailers.is_empty()
        && !decoder.remaining().is_empty()
        && let Ok(msg) = DynamicMessage::decode(desc.clone(), decoder.remaining())
    {
        messages.push(dynamic_message_to_json(&msg));
        offsets_ms.push(started.elapsed().as_millis() as u64);
    }

    Ok((messages, offsets_ms, trailers, error))
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

fn encode_connect_envelope(data: &[u8], end_stream: bool) -> Vec<u8> {
    encode_connect_envelope_compressed(data, end_stream, false)
        .expect("uncompressed envelope encoding is infallible")
}

fn encode_connect_envelope_compressed(
    data: &[u8],
    end_stream: bool,
    compress: bool,
) -> Result<Vec<u8>> {
    let (compressed_bit, payload) = if compress {
        (0x01u8, gzip_compress(data)?)
    } else {
        (0x00u8, data.to_vec())
    };
    let mut flags = compressed_bit;
    if end_stream {
        flags |= 0x02;
    }
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(payload.len() + 5);
    buf.push(flags);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

fn parse_connect_end_stream(
    payload: &[u8],
    trailers: &mut HashMap<String, String>,
    error: &mut Option<GrpcError>,
) {
    let Ok(v) = serde_json::from_slice::<Value>(payload) else {
        return;
    };

    if let Some(meta) = v.get("metadata").and_then(|m| m.as_object()) {
        for (k, val) in meta {
            let joined = match val {
                Value::Array(a) => a
                    .iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| x.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            trailers.insert(k.to_ascii_lowercase(), joined);
        }
    }

    let err_obj = v.get("error").unwrap_or(&v);
    if err_obj.get("code").and_then(|c| c.as_str()).is_some() {
        *error = Some(connect_error_from_json(err_obj));
    }
}

fn push_connect_frame(
    flags: u8,
    payload: &[u8],
    output_desc: Option<&MessageDescriptor>,
    headers: &HashMap<String, String>,
    messages: &mut Vec<Value>,
    trailers: &mut HashMap<String, String>,
    error: &mut Option<GrpcError>,
) {
    let is_end_stream = flags & 0x02 != 0;
    if is_end_stream && payload.is_empty() {
        if let Some(status) = headers.get("grpc-status").filter(|s| *s != "0") {
            let msg = headers.get("grpc-message").cloned().unwrap_or_default();
            *error = Some(trailer_status_error(status, msg));
        }
    } else if is_end_stream {
        if serde_json::from_slice::<Value>(payload).is_ok() {
            parse_connect_end_stream(payload, trailers, error);
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

fn connect_unframed_fallback(
    tail: &[u8],
    output_desc: Option<&MessageDescriptor>,
    messages: &mut Vec<Value>,
) {
    if tail.len() < 5 || tail[0] & 0x02 != 0 {
        return;
    }
    let payload = &tail[5..];
    if let Some(desc) = output_desc {
        if let Ok(msg) = DynamicMessage::decode(desc.clone(), payload) {
            messages.push(dynamic_message_to_json(&msg));
        }
    } else if let Ok(val) = serde_json::from_slice(payload) {
        messages.push(val);
    }
}

fn parse_connect_framed(
    data: &[u8],
    output_desc: Option<&MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let mut decoder = FrameDecoder::new();
    decoder.extend(data);

    while let Some((flags, payload)) = decoder.next_frame() {
        push_connect_frame(
            flags,
            &payload,
            output_desc,
            headers,
            &mut messages,
            &mut trailers,
            &mut error,
        );
    }

    if messages.is_empty() && trailers.is_empty() && error.is_none() {
        connect_unframed_fallback(decoder.remaining(), output_desc, &mut messages);
    }

    (messages, trailers, error)
}

async fn parse_connect_stream<S>(
    mut chunks: S,
    output_desc: Option<&MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> Result<(
    Vec<Value>,
    Vec<u64>,
    HashMap<String, String>,
    Option<GrpcError>,
)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut decoder = FrameDecoder::new();
    let mut messages = Vec::new();
    let mut offsets_ms = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    let started = std::time::Instant::now();

    while let Some(chunk) = chunks.next().await {
        decoder.extend(&chunk?);
        while let Some((flags, payload)) = decoder.next_frame() {
            push_connect_frame(
                flags,
                &payload,
                output_desc,
                headers,
                &mut messages,
                &mut trailers,
                &mut error,
            );
            let arrived = started.elapsed().as_millis() as u64;
            while offsets_ms.len() < messages.len() {
                offsets_ms.push(arrived);
            }
        }
    }

    if messages.is_empty() && trailers.is_empty() && error.is_none() {
        connect_unframed_fallback(decoder.remaining(), output_desc, &mut messages);
        let arrived = started.elapsed().as_millis() as u64;
        while offsets_ms.len() < messages.len() {
            offsets_ms.push(arrived);
        }
    }

    Ok((messages, offsets_ms, trailers, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_timeout_honors_explicit_small_values() {
        assert_eq!(effective_request_timeout_secs(1), 1);
        assert_eq!(effective_request_timeout_secs(3), 3);
        assert_eq!(effective_request_timeout_secs(60), 60);
    }

    #[test]
    fn request_timeout_applies_default_only_when_unset() {
        assert_eq!(effective_request_timeout_secs(0), DEFAULT_HTTP_TIMEOUT_SECS);
    }

    #[test]
    fn parse_grpc_web_trailers_handles_no_space_after_colon() {
        let mut trailers = HashMap::new();
        let mut error = None;
        parse_grpc_web_trailers(
            b"grpc-status:0\r\ngrpc-message:ok\r\n",
            &mut trailers,
            &mut error,
        );
        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("0"));
        assert_eq!(trailers.get("grpc-message").map(String::as_str), Some("ok"));
        assert!(error.is_none(), "status 0 is not an error");

        let mut trailers = HashMap::new();
        let mut error = None;
        parse_grpc_web_trailers(
            b"grpc-status:5\r\ngrpc-message:boom\r\n",
            &mut trailers,
            &mut error,
        );
        let err = error.expect("non-zero status is an error");
        assert_eq!(err.code, 5);
        assert_eq!(err.message, "boom");
    }

    #[test]
    fn header_only_error_builds_structured_grpc_error() {
        let mut headers = ResponseHeaders::new();
        headers.insert("grpc-status".to_string(), "5".to_string());
        headers.insert("grpc-message".to_string(), "greeting not found".to_string());
        headers.insert(
            "grpc-status-details-bin".to_string(),
            "CAUSBGJvb20".to_string(),
        );

        let mut trailers = HashMap::new();
        let mut error = None;
        apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);

        let err = error.expect("header-only status must yield a structured error");
        assert_eq!(err.code, 5, "NotFound = 5");
        assert!(
            err.message.contains("greeting not found"),
            "message: {}",
            err.message
        );
        assert_eq!(
            err.details,
            vec![0x08, 0x05, 0x12, 0x04, b'b', b'o', b'o', b'm']
        );
        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("5"));
    }

    #[test]
    fn header_trailers_do_not_override_in_body_trailers() {
        let mut headers = ResponseHeaders::new();
        headers.insert("grpc-status".to_string(), "5".to_string());

        let mut trailers = HashMap::from([("grpc-status".to_string(), "0".to_string())]);
        let mut error = None;
        apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);

        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("0"));
        assert!(error.is_none(), "in-body status 0 wins over header status");
    }

    const TEST_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBfTCCASOgAwIBAgIUWcL1fmtrrhRDH/YETZY49ueE6y0wCgYIKoZIzj0EAwIw
FDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDcxOTIwNTI1NloXDTM2MDcxNjIw
NTI1NlowFDESMBAGA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0D
AQcDQgAEEwvceEwaf4E5gBriB1ihbxAa16YERt+/hiIoxPx0E/+uiOEbtTllRxiG
3kXeO3tDitmuOzsSMy25dN+Mf3Y8G6NTMFEwHQYDVR0OBBYEFDzduoo6/sV0c8vW
YSamEHiJ6ph2MB8GA1UdIwQYMBaAFDzduoo6/sV0c8vWYSamEHiJ6ph2MA8GA1Ud
EwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDSAAwRQIgY24J4OIquMyFV5Oaa/iaiPjW
hpDIqr4vdj9UlPaR2xkCIQC5ZTBBiDYr+kXy5QEiqaIuoi75YB8ReyMwL2dMFyxd
rw==
-----END CERTIFICATE-----
";
    const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTWoLVyWn4NUGGIdX
a9iy8oFRmGwJBQb5oxLGtdLhWOyhRANCAAQTC9x4TBp/gTmAGuIHWKFvEBrXpgRG
37+GIijE/HQT/66I4Ru1OWVHGIbeRd47e0OK2a47OxIzLbl034x/djwb
-----END PRIVATE KEY-----
";

    fn tls_test_config(tls: crate::grpc::client::TlsConfig) -> GrpcClientConfig {
        GrpcClientConfig {
            address: "localhost:8080".to_string(),
            tls_config: Some(tls),
            ..Default::default()
        }
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_http_client_with_ca_cert() {
        let dir = tempfile::tempdir().unwrap();
        let ca_path = dir.path().join("ca.pem");
        std::fs::write(&ca_path, TEST_CERT_PEM).unwrap();

        let config = tls_test_config(crate::grpc::client::TlsConfig {
            ca_cert_path: Some(ca_path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let result = build_http_client(&config);
        assert!(
            result.is_ok(),
            "CA cert should be applied: {:?}",
            result.err()
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_http_client_with_client_identity() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("client.pem");
        let key_path = dir.path().join("client.key");
        std::fs::write(&cert_path, TEST_CERT_PEM).unwrap();
        std::fs::write(&key_path, TEST_KEY_PEM).unwrap();

        let config = tls_test_config(crate::grpc::client::TlsConfig {
            client_cert_path: Some(cert_path.to_string_lossy().into_owned()),
            client_key_path: Some(key_path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let result = build_http_client(&config);
        assert!(
            result.is_ok(),
            "client identity should be applied: {:?}",
            result.err()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn build_http_client_unreadable_ca_fails() {
        let config = tls_test_config(crate::grpc::client::TlsConfig {
            ca_cert_path: Some("/nonexistent/ca.pem".to_string()),
            ..Default::default()
        });
        let err = build_http_client(&config).expect_err("missing CA file must fail");
        assert!(
            err.to_string().contains("/nonexistent/ca.pem"),
            "error should name the file: {}",
            err
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn build_http_client_invalid_ca_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ca_path = dir.path().join("ca.pem");
        std::fs::write(
            &ca_path,
            "-----BEGIN CERTIFICATE-----\n!!!not-base64!!!\n-----END CERTIFICATE-----\n",
        )
        .unwrap();

        let config = tls_test_config(crate::grpc::client::TlsConfig {
            ca_cert_path: Some(ca_path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let err = build_http_client(&config).expect_err("corrupt CA must fail");
        assert!(
            err.to_string().contains("Invalid CA certificate")
                || err.to_string().contains("TLS configuration invalid"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn build_http_client_cert_without_key_fails() {
        let config = tls_test_config(crate::grpc::client::TlsConfig {
            client_cert_path: Some("/tmp/whatever.pem".to_string()),
            ..Default::default()
        });
        let err = build_http_client(&config).expect_err("cert without key must fail");
        assert!(
            err.to_string()
                .contains("client_cert_path and client_key_path"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn build_http_client_insecure_ok() {
        let config = tls_test_config(crate::grpc::client::TlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        });
        build_http_client(&config).expect("client build must succeed");
    }

    #[test]
    fn http_client_cache_key_distinguishes_tls_and_timeout() {
        let plain = GrpcClientConfig::default();
        let plain2 = GrpcClientConfig::default();
        assert_eq!(
            http_client_cache_key(&plain),
            http_client_cache_key(&plain2)
        );

        let insecure = tls_test_config(crate::grpc::client::TlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        });
        assert_ne!(
            http_client_cache_key(&plain),
            http_client_cache_key(&insecure)
        );

        let slower = GrpcClientConfig {
            timeout_seconds: 99,
            ..Default::default()
        };
        assert_ne!(
            http_client_cache_key(&plain),
            http_client_cache_key(&slower)
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn http_client_cache_evicts_only_the_oldest() {
        let mut cache = BoundedHttpClientCache {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
        };
        let key_of = |id: u64| HttpClientCacheKey {
            timeout_seconds: 30,
            tls_config: None,
            connection_id: id,
        };
        for id in 0..=HTTP_CLIENT_CACHE_MAX_ENTRIES as u64 {
            cache.insert(key_of(id), reqwest::Client::new());
        }

        assert_eq!(cache.map.len(), HTTP_CLIENT_CACHE_MAX_ENTRIES);
        assert!(cache.get(&key_of(0)).is_none(), "oldest must go");
        assert!(
            cache
                .get(&key_of(HTTP_CLIENT_CACHE_MAX_ENTRIES as u64))
                .is_some(),
            "newest must stay"
        );
    }

    #[test]
    fn http_client_cache_key_separates_connection_slots() {
        let first = GrpcClientConfig {
            connection_id: 1,
            ..Default::default()
        };
        let second = GrpcClientConfig {
            connection_id: 2,
            ..Default::default()
        };
        assert_ne!(
            http_client_cache_key(&first),
            http_client_cache_key(&second)
        );
        assert_ne!(
            http_client_cache_key(&GrpcClientConfig::default()),
            http_client_cache_key(&first)
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn cached_http_client_reuses_same_config() {
        let config = GrpcClientConfig {
            address: "localhost:8080".to_string(),
            timeout_seconds: 4242,
            tls_config: Some(crate::grpc::client::TlsConfig {
                insecure_skip_verify: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let key = http_client_cache_key(&config);

        let _first = cached_http_client(&config).expect("first build ok");
        assert!(HTTP_CLIENT_CACHE.lock().unwrap().get(&key).is_some());
        let len_after_first = HTTP_CLIENT_CACHE.lock().unwrap().map.len();

        let _second = cached_http_client(&config).expect("second call reuses");
        assert_eq!(HTTP_CLIENT_CACHE.lock().unwrap().map.len(), len_after_first);
    }

    #[test]
    fn encode_connect_envelope_data() {
        let data = b"hello";
        let framed = encode_connect_envelope(data, false);
        assert_eq!(framed.len(), 10);
        assert_eq!(framed[0], 0x00);
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]);
        assert_eq!(len, 5);
        assert_eq!(&framed[5..], b"hello");
    }

    #[test]
    fn encode_connect_envelope_end_stream() {
        let data = b"x";
        let framed = encode_connect_envelope(data, true);
        assert_eq!(framed[0], 0x02);
    }

    #[test]
    fn encode_connect_envelope_empty() {
        let framed = encode_connect_envelope(b"", true);
        assert_eq!(framed.len(), 5);
        assert_eq!(framed[0], 0x02);
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]);
        assert_eq!(len, 0);
    }

    #[test]
    fn parse_grpc_web_frame_header_basic() {
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
    fn parse_grpc_web_frame_header_too_short() {
        let data = vec![0x00, 0x00, 0x00];
        let mut offset = 0;
        let result = parse_grpc_web_frame_header(&data, &mut offset);
        assert!(result.is_none());
        assert_eq!(offset, 0);
    }

    #[test]
    fn parse_grpc_web_frame_header_truncated_payload() {
        let data = vec![0x00, 0x00, 0x00, 0x00, 0x0A, b'h'];
        let mut offset = 0;
        let result = parse_grpc_web_frame_header(&data, &mut offset);
        assert!(result.is_none());
        assert_eq!(offset, 5);
    }

    #[test]
    fn parse_grpc_web_framed_json_single() {
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
    fn parse_grpc_web_framed_json_multiple() {
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
    fn parse_grpc_web_framed_json_empty() {
        let (messages, trailers, error) = parse_grpc_web_framed_json(b"");
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn parse_grpc_web_framed_json_only_trailers() {
        let trailer_data = b"grpc-status: 5\ngrpc-message: not found";
        let len = trailer_data.len() as u32;
        let mut data = vec![0x80];
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(trailer_data);

        let (messages, trailers, error) = parse_grpc_web_framed_json(&data);
        assert!(messages.is_empty());
        assert_eq!(trailers.get("grpc-status").unwrap(), "5");
        assert_eq!(trailers.get("grpc-message").unwrap(), "not found");
        let err = error.unwrap();
        assert_eq!(err.code, 5);
        assert_eq!(err.message, "not found");
    }

    #[test]
    fn parse_grpc_web_framed_json_data_then_trailers() {
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

    #[test]
    fn parse_grpc_web_trailers_case_folding() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"Grpc-Status: 3\nGRPC-MESSAGE: bad";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("grpc-status").unwrap(), "3");
        assert_eq!(trailers.get("grpc-message").unwrap(), "bad");
        assert!(error.is_some());
    }

    #[test]
    fn parse_grpc_web_trailers_mixed_case() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"Grpc-Status: 4\nGrpc-Message: deadline exceeded";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("grpc-status").unwrap(), "4");
        assert_eq!(error.unwrap().code, 4);
    }

    #[test]
    fn parse_grpc_web_trailers_success() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"grpc-status: 0";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert!(error.is_none());
    }

    #[test]
    fn parse_grpc_web_trailers_custom_metadata() {
        let mut trailers = HashMap::new();
        let mut error = None;
        let payload = b"custom-key: custom-value\nx-trace-id: abc123";
        parse_grpc_web_trailers(payload, &mut trailers, &mut error);
        assert_eq!(trailers.get("custom-key").unwrap(), "custom-value");
        assert_eq!(trailers.get("x-trace-id").unwrap(), "abc123");
        assert!(error.is_none());
    }

    #[test]
    fn parse_grpc_web_framed_proto_data_frame() {
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
    fn parse_grpc_web_framed_proto_empty() {
        let pool = make_test_descriptor_pool();
        let output = pool.get_message_by_name("test.TestResponse").unwrap();
        let (messages, trailers, error) = parse_grpc_web_framed_proto(b"", &output);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn parse_grpc_web_framed_proto_trailers() {
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

    #[test]
    fn parse_connect_framed_data_json() {
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
    fn parse_connect_framed_end_stream_error_json() {
        let err = json!({"code": "unavailable", "message": "service down"});
        let body = serde_json::to_vec(&err).unwrap();
        let framed = encode_connect_envelope(&body, true);
        let headers = HashMap::new();

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        let e = error.unwrap();
        assert_eq!(e.code, 14);
        assert_eq!(e.message, "service down");
    }

    #[test]
    fn parse_connect_framed_end_stream_empty_with_header_error() {
        let framed = encode_connect_envelope(b"", true);
        let mut headers = HashMap::new();
        headers.insert("grpc-status".to_string(), "5".to_string());
        headers.insert("grpc-message".to_string(), "not found".to_string());

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert_eq!(error.unwrap().code, 5);
    }

    #[test]
    fn parse_connect_framed_end_stream_empty_no_error() {
        let framed = encode_connect_envelope(b"", true);
        let headers = HashMap::new();

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert!(messages.is_empty());
        assert!(trailers.is_empty());
        assert!(error.is_none());
    }

    #[test]
    fn parse_connect_framed_multiple_data_frames() {
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

    #[test]
    fn dynamic_message_to_json_empty_message() {
        let pool = make_test_descriptor_pool();
        let desc = pool.get_message_by_name("test.TestResponse").unwrap();
        let val = dynamic_message_to_json(&DynamicMessage::new(desc));
        assert_eq!(val, json!({}));
    }

    #[test]
    fn extract_headers_normalizes_case() {
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

    #[test]
    fn serialize_message_produces_valid_proto() {
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
    fn serialize_message_rejects_an_unknown_field() {
        let pool = make_test_descriptor_pool();
        let input = pool.get_message_by_name("test.TestRequest").unwrap();
        let err = serialize_message(&json!({"nope": "hello"}), &input)
            .expect_err("an unknown field must not be silently dropped");
        assert!(
            err.to_string()
                .contains("Failed to serialize JSON to protobuf"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn base64_decode_roundtrip_all_pad_widths() {
        for payload in [&b""[..], b"a", b"ab", b"abc", b"abcd", b"abcde", b"abcdef"] {
            let enc = base64_encode(payload);
            assert_eq!(
                base64_decode(enc.as_bytes()).unwrap(),
                payload,
                "roundtrip failed for {:?}",
                payload
            );
        }
    }

    #[test]
    fn base64_decode_ignores_whitespace() {
        let enc = base64_encode(b"hello world");
        let with_newlines = format!("{}\n{}", &enc[..4], &enc[4..]);
        assert_eq!(
            base64_decode(with_newlines.as_bytes()).unwrap(),
            b"hello world"
        );
    }

    #[test]
    fn base64_decode_rejects_invalid() {
        assert!(base64_decode(b"@@@@").is_none());
        assert!(base64_decode(b"abc").is_none());
        assert!(base64_decode(b"ab=c").is_none());
    }

    #[test]
    fn decode_grpc_web_text_body_frames() {
        let msg = json!({"reply": "hi"});
        let body = serde_json::to_vec(&msg).unwrap();
        let mut raw = vec![0x00];
        raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
        raw.extend_from_slice(&body);
        let trailer = b"grpc-status: 0";
        raw.push(0x80);
        raw.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
        raw.extend_from_slice(trailer);

        let encoded = base64_encode(&raw).into_bytes();
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/grpc-web-text+json".to_string(),
        );

        let decoded = decode_grpc_web_body(encoded, &headers);
        assert_eq!(decoded, raw);
        let (messages, trailers, error) = parse_grpc_web_framed_json(&decoded);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["reply"], "hi");
        assert_eq!(trailers.get("grpc-status").unwrap(), "0");
        assert!(error.is_none());
    }

    #[test]
    fn decode_grpc_web_body_passthrough_binary() {
        let raw = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x42];
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/grpc-web+proto".to_string(),
        );
        assert_eq!(decode_grpc_web_body(raw.clone(), &headers), raw);
    }

    #[test]
    fn parse_connect_end_stream_error_and_metadata() {
        let end = json!({
            "error": {
                "code": "resource_exhausted",
                "message": "quota hit",
                "details": [{"type": "google.rpc.RetryInfo", "value": "Cg"}]
            },
            "metadata": {
                "x-ratelimit": ["0"],
                "x-multi": ["a", "b"]
            }
        });
        let mut trailers = HashMap::new();
        let mut error = None;
        parse_connect_end_stream(
            &serde_json::to_vec(&end).unwrap(),
            &mut trailers,
            &mut error,
        );

        assert_eq!(trailers.get("x-ratelimit").unwrap(), "0");
        assert_eq!(trailers.get("x-multi").unwrap(), "a, b");
        let e = error.unwrap();
        assert_eq!(e.code, 8, "resource_exhausted maps to 8");
        assert_eq!(e.message, "quota hit");
        let details = String::from_utf8(e.details.clone()).unwrap();
        assert!(details.contains("RetryInfo"), "details missing: {details}");
    }

    #[test]
    fn parse_connect_end_stream_metadata_only_no_error() {
        let end = json!({"metadata": {"trace-id": ["abc123"]}});
        let mut trailers = HashMap::new();
        let mut error = None;
        parse_connect_end_stream(
            &serde_json::to_vec(&end).unwrap(),
            &mut trailers,
            &mut error,
        );
        assert_eq!(trailers.get("trace-id").unwrap(), "abc123");
        assert!(error.is_none());
    }

    #[test]
    fn parse_connect_framed_streaming_end_stream_surfaces_trailers_and_error() {
        let headers = HashMap::new();
        let mut framed =
            encode_connect_envelope(&serde_json::to_vec(&json!({"n": 1})).unwrap(), false);
        let end = json!({
            "error": {"code": "not_found", "message": "gone"},
            "metadata": {"x-trace": ["t-1"]}
        });
        framed.extend_from_slice(&encode_connect_envelope(
            &serde_json::to_vec(&end).unwrap(),
            true,
        ));

        let (messages, trailers, error) = parse_connect_framed(&framed, None, &headers);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["n"], 1);
        assert_eq!(trailers.get("x-trace").unwrap(), "t-1");
        let e = error.unwrap();
        assert_eq!(e.code, 5, "not_found maps to 5");
        assert_eq!(e.message, "gone");
    }

    #[test]
    fn split_connect_trailers_strips_the_prefix() {
        let mut headers = HashMap::new();
        headers.insert("x-channel".to_string(), "header".to_string());
        headers.insert("trailer-x-channel".to_string(), "trailer".to_string());
        headers.insert("trailer-x-audit".to_string(), "done".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());

        let (leading, trailers) = split_connect_trailers(headers);

        assert_eq!(leading.get("x-channel"), Some(&"header".to_string()));
        assert!(!leading.contains_key("trailer-x-channel"));
        assert!(!leading.contains_key("content-type"));

        assert_eq!(trailers.get("x-channel"), Some(&"trailer".to_string()));
        assert_eq!(trailers.get("x-audit"), Some(&"done".to_string()));
    }

    #[test]
    fn split_connect_trailers_keeps_a_bare_prefix_as_a_header() {
        let mut headers = HashMap::new();
        headers.insert("trailer-".to_string(), "odd".to_string());

        let (leading, trailers) = split_connect_trailers(headers);

        assert_eq!(leading.get("trailer-"), Some(&"odd".to_string()));
        assert!(trailers.is_empty());
    }

    #[test]
    fn public_response_headers_strips_framing_headers() {
        let mut headers = HashMap::new();
        headers.insert("grpc-status".to_string(), "0".to_string());
        headers.insert("grpc-message".to_string(), "ok".to_string());
        headers.insert(
            "content-type".to_string(),
            "application/grpc-web+proto".to_string(),
        );
        headers.insert("content-length".to_string(), "42".to_string());
        headers.insert("x-custom".to_string(), "keep".to_string());

        let public = public_response_headers(headers);
        assert_eq!(public.get("x-custom").unwrap(), "keep");
        assert!(!public.contains_key("grpc-status"));
        assert!(!public.contains_key("content-type"));
        assert!(!public.contains_key("content-length"));
    }

    #[test]
    fn connect_error_json_builds_structured_grpc_error() {
        let err = json!({
            "code": "resource_exhausted",
            "message": "quota exceeded",
            "details": [{"type": "google.rpc.RetryInfo", "value": "Cg"}]
        });
        let e = connect_error_from_json(&err);
        assert_eq!(e.code, 8);
        assert_eq!(e.message, "quota exceeded");
        let expected_details = json!([{"type": "google.rpc.RetryInfo", "value": "Cg"}]).to_string();
        assert_eq!(e.details, expected_details.into_bytes());
    }

    #[test]
    fn grpc_web_trailer_and_details_build_structured_grpc_error() {
        let status_msg = "denied: code=42 message=nested details=[x]";
        let status_json = json!({
            "code": 7,
            "message": status_msg,
            "details": [{"type": "google.rpc.ErrorInfo", "reason": "X"}]
        });
        let body = serde_json::to_vec(&status_json).unwrap();
        let mut data = vec![0x00];
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&body);
        let trailer = b"grpc-status: 7\ngrpc-message: permission denied";
        data.push(0x80);
        data.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
        data.extend_from_slice(trailer);

        let (mut messages, _trailers, mut error) = parse_grpc_web_framed_json(&data);
        let te = error.clone().unwrap();
        assert_eq!(te.code, 7, "grpc-status: 7 -> 7");
        assert_eq!(te.message, "permission denied");
        enrich_grpc_web_error(&mut messages, &mut error);
        let e = error.unwrap();
        assert_eq!(e.code, 7);
        assert_eq!(e.message, status_msg, "message survives verbatim");
        let details = String::from_utf8(e.details.clone()).unwrap();
        assert!(details.contains("ErrorInfo"), "details missing: {details}");
        assert!(messages.is_empty(), "status data frame consumed");
    }

    #[test]
    fn error_message_containing_code_marker_survives_verbatim() {
        let nasty = "bad request: code=42 message=nested details=[inline]";
        let err = json!({
            "code": "invalid_argument",
            "message": nasty,
            "details": [{"k": "v"}]
        });
        let e = connect_error_from_json(&err);
        assert_eq!(e.code, 3);
        assert_eq!(e.message, nasty, "message must survive verbatim");
        assert_eq!(e.details, json!([{"k": "v"}]).to_string().into_bytes());

        let end = json!({"error": {"code": "internal", "message": nasty}});
        let mut trailers = HashMap::new();
        let mut error = None;
        parse_connect_end_stream(
            &serde_json::to_vec(&end).unwrap(),
            &mut trailers,
            &mut error,
        );
        let e = error.unwrap();
        assert_eq!(e.code, 13);
        assert_eq!(e.message, nasty);
    }

    fn encode_status_proto(code: u8, message: &str) -> Vec<u8> {
        let mut buf = vec![0x08, code];
        buf.push(0x12);
        buf.push(message.len() as u8);
        buf.extend_from_slice(message.as_bytes());
        buf
    }

    fn grpc_web_trailer_frame(text: &str) -> Vec<u8> {
        let mut data = vec![0x80];
        data.extend_from_slice(&(text.len() as u32).to_be_bytes());
        data.extend_from_slice(text.as_bytes());
        data
    }

    #[test]
    fn grpc_web_status_details_bin_trailer_yields_proto_details() {
        let status = encode_status_proto(9, "failed");
        let trailer = format!(
            "grpc-status: 9\ngrpc-message: failed\ngrpc-status-details-bin: {}",
            base64_encode(&status)
        );
        let data = grpc_web_trailer_frame(&trailer);

        let (_messages, trailers, error) = parse_grpc_web_framed_json(&data);
        assert_eq!(trailers.get("grpc-status").unwrap(), "9");
        let e = error.unwrap();
        assert_eq!(e.code, 9);
        assert_eq!(e.details, status);
    }

    #[test]
    fn grpc_web_status_details_bin_decodes_unpadded() {
        let status = encode_status_proto(5, "gone");
        let b64 = base64_encode(&status);
        let unpadded = b64.trim_end_matches('=');
        let trailer = format!("grpc-status: 5\ngrpc-status-details-bin: {}", unpadded);
        let data = grpc_web_trailer_frame(&trailer);

        let (_m, _t, error) = parse_grpc_web_framed_json(&data);
        assert_eq!(error.unwrap().details, status);
    }

    #[test]
    fn grpc_web_status_details_bin_preferred_over_data_frame() {
        let status = encode_status_proto(7, "denied");
        let status_json = json!({"code": 7, "message": "denied", "details": [{"x": "y"}]});
        let body = serde_json::to_vec(&status_json).unwrap();
        let mut data = vec![0x00];
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&body);
        let trailer = format!(
            "grpc-status: 7\ngrpc-message: denied\ngrpc-status-details-bin: {}",
            base64_encode(&status)
        );
        data.extend_from_slice(&grpc_web_trailer_frame(&trailer));

        let (mut messages, _t, mut error) = parse_grpc_web_framed_json(&data);
        enrich_grpc_web_error(&mut messages, &mut error);
        let e = error.unwrap();
        assert_eq!(e.details, status, "standard trailer details win");
        assert_eq!(
            messages.len(),
            1,
            "data frame untouched when trailer present"
        );
    }

    #[test]
    fn base64_decode_lenient_handles_padding_variants() {
        let data = b"foobar";
        let padded = base64_encode(data);
        let unpadded = padded.trim_end_matches('=');
        assert_eq!(base64_decode_lenient(padded.as_bytes()).unwrap(), data);
        assert_eq!(base64_decode_lenient(unpadded.as_bytes()).unwrap(), data);
    }

    fn grpc_web_text_config() -> GrpcClientConfig {
        GrpcClientConfig {
            protocol: WireProtocol::GrpcWeb,
            metadata: Some(HashMap::from([(
                GRPC_WEB_TEXT_FLAG.to_string(),
                "true".to_string(),
            )])),
            ..Default::default()
        }
    }

    #[test]
    fn grpc_web_text_flag_detection() {
        assert!(grpc_web_text_enabled(&grpc_web_text_config()));
        assert!(!grpc_web_text_enabled(&GrpcClientConfig::default()));
        let off = GrpcClientConfig {
            metadata: Some(HashMap::from([(
                GRPC_WEB_TEXT_FLAG.to_string(),
                "false".to_string(),
            )])),
            ..Default::default()
        };
        assert!(!grpc_web_text_enabled(&off));
    }

    #[test]
    fn grpc_web_text_request_is_base64_and_roundtrips() {
        let config = grpc_web_text_config();
        let msg = json!({"name": "x"});
        let payload = serde_json::to_vec(&msg).unwrap();

        let (body, content_type) =
            frame_grpc_web_request(payload, "application/grpc-web+json", &config).unwrap();
        assert_eq!(content_type, "application/grpc-web-text+json");

        let framed = base64_decode(&body).unwrap();
        assert_eq!(framed[0], 0x00, "uncompressed data-frame flag");

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), content_type);
        let decoded = decode_grpc_web_body(body, &headers);
        let (messages, _t, _e) = parse_grpc_web_framed_json(&decoded);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["name"], "x");
    }

    #[test]
    fn grpc_web_binary_request_default_flag_and_content_type() {
        let config = GrpcClientConfig::default();
        let payload = b"raw".to_vec();
        let (body, content_type) =
            frame_grpc_web_request(payload, "application/grpc-web+proto", &config).unwrap();
        assert_eq!(content_type, "application/grpc-web+proto");
        assert_eq!(body[0], 0x00, "uncompressed data-frame flag");
        assert_eq!(&body[5..], b"raw", "payload unframed and unencoded");
    }

    #[test]
    fn gzip_framed_message_roundtrips() {
        let original = b"hello gzip world, this string is long enough to compress";
        let compressed = gzip_compress(original).unwrap();
        let mut framed = vec![0x01];
        framed.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        framed.extend_from_slice(&compressed);

        let mut offset = 0;
        let (flags, len) = parse_grpc_web_frame_header(&framed, &mut offset).unwrap();
        assert_eq!(flags & 0x01, 0x01);
        let raw = &framed[offset - len..offset];
        let out = grpc_web_frame_data(flags, raw).unwrap();
        assert_eq!(out, original);
    }

    #[test]
    fn gzip_grpc_web_json_frame_decodes() {
        let msg = json!({"reply": "hi"});
        let body = serde_json::to_vec(&msg).unwrap();
        let compressed = gzip_compress(&body).unwrap();
        let mut data = vec![0x01];
        data.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        data.extend_from_slice(&compressed);

        let (messages, _t, _e) = parse_grpc_web_framed_json(&data);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["reply"], "hi");
    }

    #[test]
    fn gzip_request_frame_sets_compressed_flag() {
        let config = GrpcClientConfig {
            protocol: WireProtocol::GrpcWeb,
            compression: CompressionMode::Gzip,
            ..Default::default()
        };
        let payload = b"payload to compress".to_vec();
        let (body, content_type) =
            frame_grpc_web_request(payload.clone(), "application/grpc-web+proto", &config).unwrap();
        assert_eq!(content_type, "application/grpc-web+proto");
        assert_eq!(body[0], 0x01, "compressed data-frame flag set");
        let raw = &body[5..];
        assert_eq!(grpc_web_frame_data(0x01, raw).unwrap(), payload);
    }

    #[test]
    fn streaming_connect_request_frames_equal_buffered_body() {
        let reqs = vec![json!({"seq": 0}), json!({"seq": 1}), json!({"seq": 2})];
        let frames = frame_messages_connect(&reqs);

        assert_eq!(frames.len(), reqs.len());
        assert_eq!(frames.concat(), encode_multi_request(&reqs));

        let headers = HashMap::new();
        for (i, frame) in frames.iter().enumerate() {
            let (msgs, _t, _e) = parse_connect_framed(frame, None, &headers);
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0]["seq"], i);
        }
    }

    #[test]
    fn streaming_grpc_web_request_frames_equal_buffered_body() {
        let reqs = vec![json!({"n": 1}), json!({"n": 2})];
        let frames = frame_messages_grpc_web(&reqs);
        assert_eq!(frames.len(), reqs.len());
        assert!(frames.iter().all(|f| f[0] == 0x00));
        assert_eq!(frames.concat(), encode_multi_request_grpc_web(&reqs));
    }

    fn chunk_at(data: &[u8], boundaries: &[usize]) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        let mut prev = 0;
        for &b in boundaries {
            chunks.push(data[prev..b].to_vec());
            prev = b;
        }
        chunks.push(data[prev..].to_vec());
        chunks
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("current-thread runtime")
            .block_on(future)
    }

    #[test]
    fn grpc_web_stream_parse_matches_buffered_across_chunk_boundaries() {
        block_on(async {
            let mut data = Vec::new();
            for m in [json!({"seq": 0}), json!({"seq": 1})] {
                let body = serde_json::to_vec(&m).unwrap();
                data.push(0x00);
                data.extend_from_slice(&(body.len() as u32).to_be_bytes());
                data.extend_from_slice(&body);
            }
            let trailer = b"grpc-status: 0";
            data.push(0x80);
            data.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
            data.extend_from_slice(trailer);

            let (want_m, want_t, want_e) = parse_grpc_web_framed_json(&data);
            assert_eq!(want_m.len(), 2);

            for cut in 1..data.len() {
                let chunks = chunk_at(&data, &[cut]);
                let stream =
                    futures::stream::iter(chunks.into_iter().map(Ok::<Vec<u8>, anyhow::Error>));
                let (m, _o, t, e) = parse_grpc_web_stream(Box::pin(stream), None).await.unwrap();
                assert_eq!(m, want_m, "messages differ at cut {cut}");
                assert_eq!(t, want_t, "trailers differ at cut {cut}");
                assert_eq!(e.is_some(), want_e.is_some(), "error differs at cut {cut}");
            }
        });
    }

    #[test]
    fn connect_stream_parse_matches_buffered_across_chunk_boundaries() {
        block_on(async {
            let mut data = Vec::new();
            for m in [json!({"seq": 0}), json!({"seq": 1})] {
                data.extend_from_slice(&encode_connect_envelope(
                    &serde_json::to_vec(&m).unwrap(),
                    false,
                ));
            }
            let end = json!({"metadata": {"x-trace": ["t-1"]}});
            data.extend_from_slice(&encode_connect_envelope(
                &serde_json::to_vec(&end).unwrap(),
                true,
            ));

            let headers = HashMap::new();
            let (want_m, want_t, _e) = parse_connect_framed(&data, None, &headers);
            assert_eq!(want_m.len(), 2);
            assert_eq!(want_t.get("x-trace").unwrap(), "t-1");

            for cut in 1..data.len() {
                let chunks = chunk_at(&data, &[cut]);
                let stream =
                    futures::stream::iter(chunks.into_iter().map(Ok::<Vec<u8>, anyhow::Error>));
                let (m, _o, t, _e) = parse_connect_stream(Box::pin(stream), None, &headers)
                    .await
                    .unwrap();
                assert_eq!(m, want_m, "messages differ at cut {cut}");
                assert_eq!(t, want_t, "trailers differ at cut {cut}");
            }
        });
    }

    #[test]
    fn grpc_web_stream_parse_many_tiny_chunks() {
        block_on(async {
            let body = serde_json::to_vec(&json!({"reply": "hi"})).unwrap();
            let mut data = vec![0x00];
            data.extend_from_slice(&(body.len() as u32).to_be_bytes());
            data.extend_from_slice(&body);

            let chunks: Vec<Vec<u8>> = data.iter().map(|b| vec![*b]).collect();
            let stream =
                futures::stream::iter(chunks.into_iter().map(Ok::<Vec<u8>, anyhow::Error>));
            let (m, _o, _t, _e) = parse_grpc_web_stream(Box::pin(stream), None).await.unwrap();
            assert_eq!(m.len(), 1);
            assert_eq!(m[0]["reply"], "hi");
        });
    }

    #[test]
    fn connect_request_envelope_gzip_roundtrips_and_sets_flag() {
        let original = b"a connect request payload long enough to be worth compressing";
        let framed = encode_connect_envelope_compressed(original, false, true).unwrap();

        assert_eq!(framed[0] & 0x01, 0x01, "compressed flag set");
        assert_eq!(framed[0] & 0x02, 0x00, "not end-of-stream");

        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]) as usize;
        let payload = &framed[5..5 + len];
        assert_eq!(gzip_decompress(payload).unwrap(), original);

        let plain = encode_connect_envelope_compressed(original, false, false).unwrap();
        assert_eq!(plain[0], 0x00);
        assert_eq!(&plain[5..], original);
        assert_eq!(plain, encode_connect_envelope(original, false));
    }

    #[test]
    fn compression_header_matches_content_type_and_mode() {
        let gz = GrpcClientConfig {
            compression: CompressionMode::Gzip,
            ..Default::default()
        };
        assert_eq!(
            compression_header("application/grpc-web+proto", &gz),
            Some(("grpc-encoding", "gzip"))
        );
        assert_eq!(
            compression_header("application/connect+json", &gz),
            Some(("connect-content-encoding", "gzip"))
        );
        assert_eq!(
            compression_header("application/proto", &gz),
            Some(("content-encoding", "gzip"))
        );
        assert_eq!(
            compression_header("application/json", &gz),
            Some(("content-encoding", "gzip"))
        );
        assert_eq!(
            compression_header("application/proto", &GrpcClientConfig::default()),
            None
        );
    }

    #[test]
    fn maybe_gzip_request_compresses_only_when_enabled() {
        let plain = GrpcClientConfig::default();
        let body = b"unary body".to_vec();
        assert_eq!(maybe_gzip_request(body.clone(), &plain).unwrap(), body);

        let gz = GrpcClientConfig {
            compression: CompressionMode::Gzip,
            ..Default::default()
        };
        let compressed = maybe_gzip_request(body.clone(), &gz).unwrap();
        assert_ne!(compressed, body, "gzip should transform the body");
        assert_eq!(gzip_decompress(&compressed).unwrap(), body);
    }

    #[test]
    fn frame_decoder_reassembles_split_frames() {
        let mut dec = FrameDecoder::new();
        dec.extend(&[0x00, 0x00, 0x00]);
        assert!(dec.next_frame().is_none());
        dec.extend(&[0x00, 0x02, 0xAA]);
        assert!(dec.next_frame().is_none(), "payload still short one byte");
        dec.extend(&[0xBB]);
        assert_eq!(dec.next_frame(), Some((0x00, vec![0xAA, 0xBB])));
        assert!(dec.next_frame().is_none());
        assert!(dec.remaining().is_empty());
    }

    #[test]
    fn encode_multi_request_frames_a_single_message() {
        let body = encode_multi_request(&[serde_json::json!({"name": "world"})]);
        let payload = br#"{"name":"world"}"#;

        assert_eq!(body[0], 0x00);
        assert_eq!(
            u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize,
            payload.len()
        );
        assert_eq!(&body[5..], payload);
    }

    #[test]
    fn encode_multi_request_grpc_web_frames_a_single_message() {
        let body = encode_multi_request_grpc_web(&[serde_json::json!({"name": "world"})]);
        let payload = br#"{"name":"world"}"#;

        assert_eq!(body[0], 0x00);
        assert_eq!(
            u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize,
            payload.len()
        );
        assert_eq!(&body[5..], payload);
    }

    #[test]
    fn encode_multi_request_frames_every_message_separately() {
        let body =
            encode_multi_request(&[serde_json::json!({"a": 1}), serde_json::json!({"b": 2})]);
        let mut decoder = FrameDecoder::new();
        decoder.extend(&body);

        assert_eq!(decoder.next_frame(), Some((0x00, br#"{"a":1}"#.to_vec())));
        assert_eq!(decoder.next_frame(), Some((0x00, br#"{"b":2}"#.to_vec())));
        assert!(decoder.next_frame().is_none());
    }

    #[test]
    fn encode_multi_request_never_sets_the_end_stream_flag() {
        let body = encode_multi_request(&[serde_json::json!({"a": 1})]);
        let mut decoder = FrameDecoder::new();
        decoder.extend(&body);

        while let Some((flags, _)) = decoder.next_frame() {
            assert_eq!(flags & 0x02, 0);
        }
        assert!(decoder.remaining().is_empty());
    }
    #[test]
    fn a_stream_reports_an_arrival_offset_per_message() {
        block_on(async {
            let mut data = Vec::new();
            for m in [json!({"seq": 0}), json!({"seq": 1})] {
                let body = serde_json::to_vec(&m).unwrap();
                data.push(0x00);
                data.extend_from_slice(&(body.len() as u32).to_be_bytes());
                data.extend_from_slice(&body);
            }
            let stream = futures::stream::iter(vec![Ok::<Vec<u8>, anyhow::Error>(data)]);
            let (messages, offsets, _t, _e) =
                parse_grpc_web_stream(Box::pin(stream), None).await.unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(offsets.len(), messages.len(), "one offset per message");
            assert!(offsets[0] <= offsets[1], "offsets are monotonic");
        });
    }

    #[tokio::test]
    async fn a_body_past_the_memory_cap_is_refused_before_it_is_held() {
        let piece = vec![0u8; 8 * 1024 * 1024];
        let chunks: Vec<Result<Vec<u8>>> = (0..5).map(|_| Ok(piece.clone())).collect();
        let err = collect_stream(Box::pin(futures::stream::iter(chunks)))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("32 MiB"), "{err}");

        let small = collect_stream(Box::pin(futures::stream::iter(vec![
            Ok::<Vec<u8>, anyhow::Error>(vec![1, 2]),
            Ok(vec![3]),
        ])))
        .await
        .unwrap();
        assert_eq!(small, vec![1, 2, 3]);
    }
}
