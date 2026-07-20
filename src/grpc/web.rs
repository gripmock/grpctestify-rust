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

/// Response metadata a `.gctf` test can assert on via `@header(...)`: everything
/// except the framing-level `grpc-*` status headers and HTTP content headers.
fn public_response_headers(headers: ResponseHeaders) -> ResponseHeaders {
    headers
        .into_iter()
        .filter(|(k, _)| !k.starts_with("grpc-") && k != "content-type" && k != "content-length")
        .collect()
}

/// True when the response is gRPC-Web text framing (`application/grpc-web-text`),
/// in which case the entire body — data and trailer frames alike — is base64.
fn is_grpc_web_text(headers: &ResponseHeaders) -> bool {
    headers
        .get("content-type")
        .is_some_and(|c| c.contains("grpc-web-text"))
}

/// Undo gRPC-Web text (base64) framing when the server used it, yielding the raw
/// length-prefixed frame stream. Binary gRPC-Web bodies pass through unchanged;
/// undecodable text bodies fall back to the raw bytes rather than erroring.
fn decode_grpc_web_body(body: Vec<u8>, headers: &ResponseHeaders) -> Vec<u8> {
    if is_grpc_web_text(headers) {
        base64_decode(&body).unwrap_or(body)
    } else {
        body
    }
}

/// Decode a standard-alphabet base64 body, ignoring ASCII whitespace/newlines.
/// gRPC-Web text encodes the whole response stream as one base64 blob; returns
/// `None` on any invalid input so the caller can fall back to the raw bytes.
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
                return None; // data after padding
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
        return None; // truncated (non-multiple-of-4) input
    }
    Some(out)
}

/// Standard-alphabet base64 encoder (with `=` padding). Used to emit
/// gRPC-Web text (base64) request bodies.
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

/// Decode base64 that MAY omit padding: gRPC binary metadata (`-bin` headers
/// such as `grpc-status-details-bin`) is base64 and frequently sent unpadded.
/// Whitespace is ignored; the tail is re-padded so the strict [`base64_decode`]
/// does the actual decoding.
fn base64_decode_lenient(input: &[u8]) -> Option<Vec<u8>> {
    let mut cleaned: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    match cleaned.len() % 4 {
        0 => {}
        1 => return None, // invalid base64 length
        r => cleaned.resize(cleaned.len() + (4 - r), b'='),
    }
    base64_decode(&cleaned)
}

/// gzip (RFC 1952) a message payload — the on-wire encoding gRPC calls `gzip`.
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

/// gunzip a gzip-compressed message payload.
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

/// Incremental decoder for the 5-byte length-prefixed frames shared by gRPC-Web
/// and Connect (`[flags:1][len:4][payload:len]`). Bytes are appended as they
/// arrive off the wire; complete frames are popped one at a time, so a framed
/// body split at arbitrary chunk boundaries decodes to the same frames as the
/// whole-buffer parsers.
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

    /// Pop the next fully-buffered frame, or `None` while the head frame is still
    /// incomplete (needs more chunks).
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

    /// Bytes buffered but not yet forming a complete frame — used for the
    /// unframed-body fallback the buffered proto/connect parsers also apply.
    fn remaining(&self) -> &[u8] {
        &self.buf
    }
}

/// Request-compression header for a content type when `config` selects gzip.
///
/// Three wire conventions coexist: gRPC-Web advertises message compression with
/// `grpc-encoding`; Connect *streaming* uses `connect-content-encoding` (with a
/// per-envelope compressed flag); Connect *unary* uses standard HTTP
/// `content-encoding` on the whole body. Returns `None` when uncompressed
/// (the default) or for content types that carry no compression.
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

/// Gzip a Connect *unary* request body when compression is enabled — the whole
/// body is compressed and advertised with `content-encoding: gzip` (added by the
/// send layer). No-op when uncompressed (the default). Streaming Connect uses
/// per-envelope compression instead (see [`encode_connect_envelope_compressed`]).
fn maybe_gzip_request(body: Vec<u8>, config: &GrpcClientConfig) -> Result<Vec<u8>> {
    if config.compression == CompressionMode::Gzip {
        gzip_compress(&body)
    } else {
        Ok(body)
    }
}

/// Wrap a sequence of pre-framed request messages as a streaming reqwest body:
/// each frame is yielded as its own chunk, so the request is sent incrementally
/// (frame-by-frame) rather than buffered into one blob. This is the client
/// side of true client-streaming/bidi; how much interleaving is actually
/// realized still depends on the server and HTTP version (see module notes).
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
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub error: Option<GrpcError>,
}

/// Map a gRPC status code token to its numeric code. Connect reports the code as
/// a lowercase name (e.g. `"unavailable"`); grpc-web reports it numerically. A
/// numeric token is parsed directly; an unrecognized token is UNKNOWN(2).
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

/// Build a structured [`GrpcError`] from a Connect error JSON object
/// `{ "code": <string>, "message": <string>, "details"?: [ … ] }`.
///
/// `details` for the HTTP protocols is the JSON detail array serialized to bytes
/// verbatim (e.g. `[{"type":"…","value":"…"}]`), empty when absent — the runner
/// decodes it as raw JSON-array bytes (`decode_json_details`). This is distinct
/// from the tonic/grpc path, where `details` is the proto-encoded
/// `google.rpc.Status`.
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

/// Build a structured [`GrpcError`] from grpc-web / Connect trailer status
/// (`grpc-status` numeric + `grpc-message`). No details are available at the
/// trailer level; grpc-web details ride in a data frame (see
/// [`enrich_grpc_web_error`]).
fn trailer_status_error(code_token: &str, message: String) -> GrpcError {
    GrpcError::new(grpc_code_from_token(code_token), message)
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

    // Try parsing as plain JSON first; fall back to Connect envelope framing
    // if the server treated our unframed request as streaming.
    match serde_json::from_slice::<Value>(&response_bytes) {
        Ok(v) => {
            let trailers = HashMap::new();
            let mut error = None;
            if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
                let msg = headers.get("grpc-message").cloned().unwrap_or_default();
                error = Some(trailer_status_error(grpc_status, msg));
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

    let trailers = HashMap::new();
    let mut error = None;
    if let Some(grpc_status) = headers.get("grpc-status").filter(|s| *s != "0") {
        let msg = headers.get("grpc-message").cloned().unwrap_or_default();
        error = Some(trailer_status_error(grpc_status, msg));
    }
    let response_headers = public_response_headers(headers);

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
    let compress = config.compression == CompressionMode::Gzip;
    let framed = encode_connect_envelope_compressed(&body, true, compress)?;

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

    let (messages, trailers, error) =
        parse_connect_stream(&mut body_stream, None, &headers).await?;
    Ok(WebResponse {
        messages,
        headers: public_response_headers(headers),
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
    let compress = config.compression == CompressionMode::Gzip;
    let framed = encode_connect_envelope_compressed(&request_bytes, true, compress)?;

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

    let (messages, trailers, error) =
        parse_connect_stream(&mut body_stream, Some(output_desc), &headers).await?;
    Ok(WebResponse {
        messages,
        headers: public_response_headers(headers),
        trailers,
        error,
    })
}

/// Request-metadata flag that opts a gRPC-Web call into text (base64) request
/// framing. Set it from a `.gctf` OPTIONS/header section, e.g.
/// `header: "grpc-web-text: true"`. Default (absent/false) sends binary
/// `application/grpc-web+proto`/`+json`. The flag is consumed by the transport
/// and never forwarded as an HTTP header (see [`send_http_post`]).
const GRPC_WEB_TEXT_FLAG: &str = "grpc-web-text";

/// Whether the configured request metadata opts into gRPC-Web text framing.
fn grpc_web_text_enabled(config: &GrpcClientConfig) -> bool {
    config.metadata.as_ref().is_some_and(|m| {
        m.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case(GRPC_WEB_TEXT_FLAG)
                && (v.trim().eq_ignore_ascii_case("true") || v.trim() == "1")
        })
    })
}

/// Frame a single gRPC-Web request message and pick its content type.
///
/// Applies per-message gzip when `config.compression` is `Gzip` (compressed
/// frame flag `0x01`; the caller-side `grpc-encoding: gzip` header is added in
/// [`send_http_post`]), prefixes the standard 5-byte length header, and — when
/// [`grpc_web_text_enabled`] — base64-encodes the whole frame and switches the
/// content type to the `grpc-web-text` variant. `base_content_type` is the
/// binary type, e.g. `application/grpc-web+proto` or `application/grpc-web+json`.
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

/// Proper gRPC-Web JSON: `application/grpc-web+json` with 5-byte framing.
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

    let (mut messages, mut trailers, mut error) =
        parse_grpc_web_response(&mut body_stream, None, &headers).await?;
    apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);
    enrich_grpc_web_error(&mut messages, &mut error);

    let response_headers = public_response_headers(headers);

    Ok(WebResponse {
        messages,
        headers: response_headers,
        trailers,
        error,
    })
}

/// Read a gRPC-Web response body, incrementally when possible. Binary framing is
/// frame-decoded as chunks arrive ([`parse_grpc_web_stream`]); gRPC-Web *text*
/// (`grpc-web-text`) encodes the whole stream as one base64 blob and so must be
/// buffered before decoding — an honest limit of that framing.
async fn parse_grpc_web_response<S>(
    body_stream: &mut S,
    output_desc: Option<&MessageDescriptor>,
    headers: &ResponseHeaders,
) -> Result<(Vec<Value>, HashMap<String, String>, Option<GrpcError>)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    if is_grpc_web_text(headers) {
        let raw = collect_stream(body_stream).await?;
        let framed = decode_grpc_web_body(raw, headers);
        Ok(match output_desc {
            Some(desc) => parse_grpc_web_framed_proto(&framed, desc),
            None => parse_grpc_web_framed_json(&framed),
        })
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

    let (messages, mut trailers, mut error) =
        parse_grpc_web_response(&mut body_stream, Some(output_desc), &headers).await?;
    apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);
    Ok(WebResponse {
        messages,
        headers: public_response_headers(headers),
        trailers,
        error,
    })
}

/// Frame multiple requests as a ConnectRPC envelope stream: one data frame per
/// message followed by the empty end-of-stream frame. Returned per-frame so the
/// same frames can either be concatenated into a buffered body
/// ([`encode_multi_request`]) or streamed one chunk at a time ([`frames_to_body`]).
fn frame_messages_connect(requests: &[Value]) -> Vec<Vec<u8>> {
    let mut frames: Vec<Vec<u8>> = requests
        .iter()
        .map(|req| {
            let body = serde_json::to_vec(req).unwrap_or_default();
            encode_connect_envelope(&body, false)
        })
        .collect();
    frames.push(encode_connect_envelope(b"", true));
    frames
}

/// Frame multiple requests as gRPC-Web data frames (flag 0x00), one per message.
/// No explicit end-of-stream frame — connection close signals end in gRPC-Web.
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

/// Encode multiple requests as a ConnectRPC envelope stream (buffered).
pub(crate) fn encode_multi_request(requests: &[Value]) -> Vec<u8> {
    frame_messages_connect(requests).concat()
}

/// Encode multiple requests as a gRPC-Web frame stream (buffered).
pub(crate) fn encode_multi_request_grpc_web(requests: &[Value]) -> Vec<u8> {
    frame_messages_grpc_web(requests).concat()
}

/// Public wrapper for parse_connect_framed so client.rs can use it.
pub(crate) fn parse_connect_framed_public(
    data: &[u8],
    output_desc: Option<&prost_reflect::MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    parse_connect_framed(data, output_desc, headers)
}

/// Public wrapper for parse_grpc_web_framed_json so client.rs can use it.
pub(crate) fn parse_grpc_web_framed_json_public(
    data: &[u8],
) -> (Vec<Value>, HashMap<String, String>, Option<GrpcError>) {
    parse_grpc_web_framed_json(data)
}

/// Fold structured error details from a gRPC-Web data frame into the status.
/// grpc-web carries only `grpc-status`/`grpc-message` in trailers; when the
/// server also emits a `google.rpc.Status` JSON as the last data frame (has
/// `code` + `message`), promote its numeric code/message and attach its
/// `details` array (as raw JSON-array bytes) to the [`GrpcError`], then drop the
/// consumed message frame.
pub(crate) fn enrich_grpc_web_error(messages: &mut Vec<Value>, error: &mut Option<GrpcError>) {
    // Prefer the standard `grpc-status-details-bin` trailer: if it already
    // supplied structured details, leave the data frame untouched.
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

/// Default request timeout (seconds) applied only when no timeout is configured.
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 5;

/// Resolve the HTTP request timeout for grpc-web/connect transports.
///
/// The configured timeout is honored verbatim; a floor is applied *only* when
/// the value is 0 (unset), defaulting to [`DEFAULT_HTTP_TIMEOUT_SECS`]. An
/// explicit small timeout (e.g. `OPTIONS.timeout: 1`) must never be inflated.
fn effective_request_timeout_secs(configured: u64) -> u64 {
    if configured == 0 {
        DEFAULT_HTTP_TIMEOUT_SECS
    } else {
        configured
    }
}

/// Cache key for a built reqwest client: only the fields that influence the
/// client build (TLS material + request timeout). `reqwest::Client` owns a
/// connection pool, so reusing one instance across requests enables keep-alive
/// and avoids re-reading CA/cert/key PEM from disk on every call.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpClientCacheKey {
    timeout_seconds: u64,
    tls_config: Option<TlsConfig>,
}

fn http_client_cache_key(config: &GrpcClientConfig) -> HttpClientCacheKey {
    HttpClientCacheKey {
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
    }
}

/// Upper bound on cached clients. When reached the cache is cleared; clients are
/// cheap to rebuild and this bounds growth over long runs against many configs.
const HTTP_CLIENT_CACHE_MAX_ENTRIES: usize = 64;

static HTTP_CLIENT_CACHE: LazyLock<Mutex<HashMap<HttpClientCacheKey, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Return a process-global reqwest client for this config, building (and reading
/// TLS PEM files) at most once per distinct effective config. `reqwest::Client`
/// is `Send + Sync` and cheaply cloneable — clones share one connection pool.
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
    if cache.len() >= HTTP_CLIENT_CACHE_MAX_ENTRIES {
        cache.clear();
    }
    cache.insert(key, client.clone());
    Ok(client)
}

/// Build the reqwest client for gRPC-Web / ConnectRPC calls, applying the
/// full TLS configuration: skip-verify, custom CA, and client identity (mTLS).
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

/// Resolve the POST URL for a service/method, honoring an explicit scheme in
/// `config.address` and defaulting to https when TLS is configured.
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

/// Build the POST request (URL, content type, compression header, user metadata)
/// short of the body — shared by the buffered and streaming send paths.
fn build_post_request(
    config: &GrpcClientConfig,
    url: &str,
    content_type: &str,
) -> Result<reqwest::RequestBuilder> {
    let http_client = cached_http_client(config)?;
    let mut http_req = http_client.post(url).header("Content-Type", content_type);

    // Advertise request compression with the header this content type expects
    // (grpc-encoding / connect-content-encoding / content-encoding). The body
    // is gzipped by the caller before it reaches here.
    if let Some((name, value)) = compression_header(content_type, config) {
        http_req = http_req.header(name, value);
    }

    if let Some(ref metadata) = config.metadata {
        for (k, v) in metadata {
            // `user-agent` is set on the client; `grpc-web-text` is an internal
            // transport flag, not a wire header.
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

/// Send a POST whose request body may itself be a stream ([`frames_to_body`]) and
/// return the response's status/headers plus an incremental body-chunk stream —
/// the client half of streaming: request frames go out as they are produced and
/// response frames are read as they arrive. The returned stream yields the raw
/// body bytes; frame decoding is the caller's (see [`parse_grpc_web_stream`] /
/// [`parse_connect_stream`]).
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

/// Drain a response body-chunk stream into one buffer — used for the buffered
/// sub-cases of a streaming send (HTTP errors, gRPC-Web text base64 bodies).
async fn collect_stream<S>(mut chunks: S) -> Result<Vec<u8>>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut buf = Vec::new();
    while let Some(chunk) = chunks.next().await {
        buf.extend_from_slice(&chunk?);
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
        // gRPC-Web / tonic-web emit `grpc-status:0` with NO space after the colon;
        // split on the first ':' and trim so both `key:value` and `key: value` work.
        if let Some((k, v)) = line.split_once(':') {
            trailers.insert(k.trim().to_ascii_lowercase(), percent_decode(v.trim()));
        }
    }
    if let Some(status) = trailers.get("grpc-status").filter(|s| *s != "0") {
        let msg = trailers.get("grpc-message").cloned().unwrap_or_default();
        let mut err = trailer_status_error(status, msg);
        // Standard gRPC-Web error details ride in the `grpc-status-details-bin`
        // trailer as base64 of a `google.rpc.Status` proto — the SAME shape the
        // tonic path produces, so `decode_status_details` handles both uniformly.
        if let Some(details_b64) = trailers.get("grpc-status-details-bin")
            && let Some(bytes) = base64_decode_lenient(details_b64.as_bytes())
        {
            err.details = bytes;
        }
        *error = Some(err);
    }
}

/// gRPC-Web "Trailers-Only" fallback. For an immediate RPC error tonic-web (and
/// gRPC-Web generally) reply HTTP 200 with an EMPTY body and carry
/// `grpc-status`/`grpc-message`/`grpc-status-details-bin` in the HTTP RESPONSE
/// HEADERS — there is no in-body trailer frame. When the framed body yielded no
/// in-body trailers, mirror those headers into `trailers` and, for a non-zero
/// status, build the structured [`GrpcError`] (decoding `grpc-status-details-bin`
/// as the same `google.rpc.Status` base64 the in-body trailer path handles).
/// In-body trailers win: if the body already produced a `grpc-status`, do nothing.
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

/// Materialize a gRPC-Web data-frame payload, gunzipping it when the frame's
/// compressed flag (`0x01`) is set. Returns `None` for a trailer frame (`0x80`)
/// or when decompression fails.
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

/// Fold one gRPC-Web frame (JSON payloads) into the running result. A trailer
/// frame (`0x80`) updates trailers/error; a data frame is gunzipped as needed
/// and pushed as a JSON message. Shared by the buffered and streaming parsers so
/// both yield identical output.
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

/// gRPC-Web frame handler for protobuf payloads (see [`push_grpc_web_frame_json`]).
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

/// Incrementally parse a gRPC-Web response as its body chunks arrive off the
/// wire. Frames are decoded and emitted as soon as they are complete, so
/// server-streaming/bidi messages surface without buffering the whole body.
/// `output_desc` selects protobuf vs JSON decoding. Chunk boundaries are
/// irrelevant — a frame split across chunks reassembles in [`FrameDecoder`].
async fn parse_grpc_web_stream<S>(
    mut chunks: S,
    output_desc: Option<&MessageDescriptor>,
) -> Result<(Vec<Value>, HashMap<String, String>, Option<GrpcError>)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut decoder = FrameDecoder::new();
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;

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
        }
    }

    // Same unframed-body fallback the buffered proto parser applies.
    if let Some(desc) = output_desc
        && messages.is_empty()
        && trailers.is_empty()
        && !decoder.remaining().is_empty()
        && let Ok(msg) = DynamicMessage::decode(desc.clone(), decoder.remaining())
    {
        messages.push(dynamic_message_to_json(&msg));
    }

    Ok((messages, trailers, error))
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

/// Encode a ConnectRPC envelope frame.
/// Per spec: bit 0 = compressed, bit 1 = end_stream.
/// See: https://connectrpc.com/docs/protocol/#streaming-rpcs
fn encode_connect_envelope(data: &[u8], end_stream: bool) -> Vec<u8> {
    // Uncompressed path cannot fail (gzip is the only fallible branch).
    encode_connect_envelope_compressed(data, end_stream, false)
        .expect("uncompressed envelope encoding is infallible")
}

/// Encode a ConnectRPC envelope, gzipping the payload and setting the compressed
/// flag (bit 0) when `compress`. Used for Connect request compression; the
/// matching `connect-content-encoding: gzip` header is added by the send layer.
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

/// Parse a Connect end-of-stream message.
///
/// The Connect streaming protocol terminates a response with a frame (flag
/// `0x02`) carrying `{"error"?: {code,message,details}, "metadata"?: {k:[v,…]}}`.
/// `metadata` becomes trailers (so `@trailer`/`@has_trailer` work), and the
/// error — nested under `"error"` for streaming, or flat for a defensive unary
/// fallback — is carried as a structured [`GrpcError`].
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

    // Streaming errors are nested under "error"; fall back to a flat body so a
    // unary error envelope routed here is still understood.
    let err_obj = v.get("error").unwrap_or(&v);
    if err_obj.get("code").and_then(|c| c.as_str()).is_some() {
        *error = Some(connect_error_from_json(err_obj));
    }
}

/// Fold one Connect envelope into the running result. Shared by the buffered and
/// streaming parsers. End-of-stream frames (`0x02`) surface metadata as trailers
/// and the (nested or flat) error as a structured status; data frames decode as
/// protobuf when `output_desc` is set, else JSON.
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
        // End stream with no data — check headers for error.
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

/// Fallback for a server that answered a streaming request with a single raw
/// (unframed) message: reinterpret `[flags][len]` + trailing bytes as one
/// payload. Applied only when framed parsing yielded nothing.
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

/// Incremental Connect response parser (see [`parse_grpc_web_stream`]): decodes
/// envelope frames as body chunks arrive, surfacing streamed messages and the
/// end-of-stream trailers/error without buffering the whole response.
async fn parse_connect_stream<S>(
    mut chunks: S,
    output_desc: Option<&MessageDescriptor>,
    headers: &HashMap<String, String>,
) -> Result<(Vec<Value>, HashMap<String, String>, Option<GrpcError>)>
where
    S: Stream<Item = Result<Vec<u8>>> + Unpin,
{
    let mut decoder = FrameDecoder::new();
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;

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
        }
    }

    if messages.is_empty() && trailers.is_empty() && error.is_none() {
        connect_unframed_fallback(decoder.remaining(), output_desc, &mut messages);
    }

    Ok((messages, trailers, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_timeout_honors_explicit_small_values() {
        // A configured sub-5s timeout must not be inflated.
        assert_eq!(effective_request_timeout_secs(1), 1);
        assert_eq!(effective_request_timeout_secs(3), 3);
        assert_eq!(effective_request_timeout_secs(60), 60);
    }

    #[test]
    fn request_timeout_applies_default_only_when_unset() {
        // Only an unset (0) timeout falls back to the default floor.
        assert_eq!(effective_request_timeout_secs(0), DEFAULT_HTTP_TIMEOUT_SECS);
    }

    #[test]
    fn parse_grpc_web_trailers_handles_no_space_after_colon() {
        // Real tonic-web / gRPC-Web emit `grpc-status:0` with NO space after ':'.
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

        // A non-zero no-space status trailer must still build the error.
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
        // Trailers-Only response: empty body, grpc-* status in the HTTP headers.
        let mut headers = ResponseHeaders::new();
        headers.insert("grpc-status".to_string(), "5".to_string());
        headers.insert("grpc-message".to_string(), "greeting not found".to_string());
        // Unpadded base64 of a google.rpc.Status (code=5, message="boom").
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
        // Status also surfaces as a trailer so `@trailer("grpc-status")` sees it.
        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("5"));
    }

    #[test]
    fn header_trailers_do_not_override_in_body_trailers() {
        // In-body trailer frame already carried the status: headers must not clobber it.
        let mut headers = ResponseHeaders::new();
        headers.insert("grpc-status".to_string(), "5".to_string());

        let mut trailers = HashMap::from([("grpc-status".to_string(), "0".to_string())]);
        let mut error = None;
        apply_grpc_web_header_trailers(&headers, &mut trailers, &mut error);

        assert_eq!(trailers.get("grpc-status").map(String::as_str), Some("0"));
        assert!(error.is_none(), "in-body status 0 wins over header status");
    }

    // Self-signed test certificate (CN=localhost, EC P-256) and its PKCS#8 key.
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
    fn test_build_http_client_with_ca_cert() {
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
    fn test_build_http_client_with_client_identity() {
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
    fn test_build_http_client_unreadable_ca_fails() {
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
    fn test_build_http_client_invalid_ca_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ca_path = dir.path().join("ca.pem");
        // PEM framing with corrupt base64 payload.
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
        // reqwest defers certificate parsing to build(); the error must still
        // point the user at the TLS files.
        assert!(
            err.to_string().contains("Invalid CA certificate")
                || err.to_string().contains("TLS configuration invalid"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_build_http_client_cert_without_key_fails() {
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
    fn test_build_http_client_insecure_ok() {
        let config = tls_test_config(crate::grpc::client::TlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        });
        assert!(build_http_client(&config).is_ok());
    }

    #[test]
    fn http_client_cache_key_distinguishes_tls_and_timeout() {
        let plain = GrpcClientConfig::default();
        let plain2 = GrpcClientConfig::default();
        // Same effective config -> same key (client would be reused).
        assert_eq!(
            http_client_cache_key(&plain),
            http_client_cache_key(&plain2)
        );

        // Differing insecure flag -> different key.
        let insecure = tls_test_config(crate::grpc::client::TlsConfig {
            insecure_skip_verify: true,
            ..Default::default()
        });
        assert_ne!(
            http_client_cache_key(&plain),
            http_client_cache_key(&insecure)
        );

        // Differing timeout -> different key.
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
    fn cached_http_client_reuses_same_config() {
        // A unique timeout keeps this key from colliding with other tests that
        // may populate the shared cache.
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
        assert!(HTTP_CLIENT_CACHE.lock().unwrap().contains_key(&key));
        let len_after_first = HTTP_CLIENT_CACHE.lock().unwrap().len();

        // A second call with the same config must hit the cache — no new entry,
        // no PEM re-read, no rebuild.
        let _second = cached_http_client(&config).expect("second call reuses");
        assert_eq!(HTTP_CLIENT_CACHE.lock().unwrap().len(), len_after_first);
    }

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
        let err = error.unwrap();
        assert_eq!(err.code, 5);
        assert_eq!(err.message, "not found");
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
        assert_eq!(error.unwrap().code, 4);
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
        let e = error.unwrap();
        assert_eq!(e.code, 14);
        assert_eq!(e.message, "service down");
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
        assert_eq!(error.unwrap().code, 5);
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

    #[test]
    fn test_dynamic_message_to_json_empty_message() {
        let pool = make_test_descriptor_pool();
        let desc = pool.get_message_by_name("test.TestResponse").unwrap();
        let val = dynamic_message_to_json(&DynamicMessage::new(desc));
        // Empty message serializes to {} not null
        assert_eq!(val, json!({}));
    }

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

    #[test]
    fn test_base64_decode_roundtrip_all_pad_widths() {
        for payload in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
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
    fn test_base64_decode_ignores_whitespace() {
        let enc = base64_encode(b"hello world");
        let with_newlines = format!("{}\n{}", &enc[..4], &enc[4..]);
        assert_eq!(
            base64_decode(with_newlines.as_bytes()).unwrap(),
            b"hello world"
        );
    }

    #[test]
    fn test_base64_decode_rejects_invalid() {
        assert!(base64_decode(b"@@@@").is_none()); // invalid alphabet
        assert!(base64_decode(b"abc").is_none()); // truncated (not multiple of 4)
        assert!(base64_decode(b"ab=c").is_none()); // data after padding
    }

    #[test]
    fn test_decode_grpc_web_text_body_frames() {
        // Build a binary gRPC-Web stream: one JSON data frame + a trailer frame,
        // then base64 the whole thing as `application/grpc-web-text` does.
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
    fn test_decode_grpc_web_body_passthrough_binary() {
        let raw = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x42];
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/grpc-web+proto".to_string(),
        );
        // Binary framing is not base64 — must pass through untouched.
        assert_eq!(decode_grpc_web_body(raw.clone(), &headers), raw);
    }

    #[test]
    fn test_parse_connect_end_stream_error_and_metadata() {
        // Connect streaming end-of-stream: error nested under "error",
        // trailers carried in "metadata" (values are arrays of strings).
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
        // details are the JSON detail array serialized to bytes, verbatim.
        let details = String::from_utf8(e.details.clone()).unwrap();
        assert!(details.contains("RetryInfo"), "details missing: {details}");
    }

    #[test]
    fn test_parse_connect_end_stream_metadata_only_no_error() {
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
    fn test_parse_connect_framed_streaming_end_stream_surfaces_trailers_and_error() {
        let headers = HashMap::new();
        // A data frame, then a Connect end-of-stream frame with nested error + metadata.
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
    fn test_public_response_headers_strips_framing_headers() {
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
        // A Connect error JSON with details must yield a GrpcError with the
        // numeric code, the message, and the detail array as raw JSON bytes —
        // no string round-trip.
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
        // grpc-web reports the status numerically in the trailer frame, with the
        // structured google.rpc.Status (code/message/details) as a data frame.
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
        // Trailer frame yields the numeric code + trailer message.
        let te = error.clone().unwrap();
        assert_eq!(te.code, 7, "grpc-status: 7 -> 7");
        assert_eq!(te.message, "permission denied");
        // The data frame promotes the richer google.rpc.Status (verbatim message
        // with `code=` markers + details as raw JSON-array bytes).
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
        // Regression: the old string parser re-parsed a formatted
        // `code=/message=/details=[` string and corrupted any message that
        // itself contained those markers. Structurally, the message is carried
        // byte-for-byte and the code stays correct.
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

        // Same guarantee through the Connect end-of-stream parser.
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

    // GAP 1 — standard gRPC-Web error details via `grpc-status-details-bin`.

    /// Hand-encode a `google.rpc.Status` proto (field 1 = code varint,
    /// field 2 = message string) — the same shape the tonic path emits.
    fn encode_status_proto(code: u8, message: &str) -> Vec<u8> {
        let mut buf = vec![0x08, code]; // field 1 (code), varint
        buf.push(0x12); // field 2 (message), length-delimited
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
        // details are the proto-encoded google.rpc.Status bytes verbatim — the
        // SAME bytes the native gRPC path stores, so decode_status_details works.
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
        // A data frame carrying a google.rpc.Status JSON (the legacy heuristic
        // source) plus a standard grpc-status-details-bin trailer: the trailer
        // wins and the data frame is left intact.
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

    // GAP 2 — gRPC-Web text (base64) REQUEST mode.

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
        // Absent flag -> binary (default).
        assert!(!grpc_web_text_enabled(&GrpcClientConfig::default()));
        // Falsey value -> binary.
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

        // The body is base64 of a normal (uncompressed) framed message.
        let framed = base64_decode(&body).unwrap();
        assert_eq!(framed[0], 0x00, "uncompressed data-frame flag");

        // Round-trip through the response decoder path (text bodies are base64).
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

    // GAP 3 — gzip compression on the HTTP transports.

    #[test]
    fn gzip_framed_message_roundtrips() {
        // compress -> frame (compressed flag) -> deframe -> decompress -> original
        let original = b"hello gzip world, this string is long enough to compress";
        let compressed = gzip_compress(original).unwrap();
        let mut framed = vec![0x01]; // per-message compressed flag
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
        // Frame body deframes+gunzips back to the original payload.
        let raw = &body[5..];
        assert_eq!(grpc_web_frame_data(0x01, raw).unwrap(), payload);
    }

    // TASK 1 — streaming request body: the incremental frame stream must equal
    // the buffered body, frame-for-frame and in order.

    #[test]
    fn streaming_connect_request_frames_equal_buffered_body() {
        let reqs = vec![json!({"seq": 0}), json!({"seq": 1}), json!({"seq": 2})];
        let frames = frame_messages_connect(&reqs);

        // One data frame per message + the empty end-of-stream frame.
        assert_eq!(frames.len(), reqs.len() + 1);
        assert_eq!(frames.last().unwrap(), &encode_connect_envelope(b"", true));
        // Concatenating the streamed frames reproduces the buffered body exactly.
        assert_eq!(frames.concat(), encode_multi_request(&reqs));

        // Each frame is one intact, in-order envelope.
        let headers = HashMap::new();
        for (i, frame) in frames.iter().take(reqs.len()).enumerate() {
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
        // Each frame is a data frame (flag 0x00) and concatenation == buffered.
        assert!(frames.iter().all(|f| f[0] == 0x00));
        assert_eq!(frames.concat(), encode_multi_request_grpc_web(&reqs));
    }

    /// Split `data` into `n`-ish byte chunks so a frame straddles boundaries.
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

    // TASK 1 — incremental response parsing: a framed body split at ARBITRARY
    // chunk boundaries decodes to the same messages/trailers as the whole-buffer
    // parser (chunk-boundary robustness).

    #[tokio::test]
    async fn grpc_web_stream_parse_matches_buffered_across_chunk_boundaries() {
        // Two JSON data frames + a trailer frame — a realistic server-stream body.
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

        // Every single-cut split (including mid-frame-header and mid-payload)
        // must reproduce the buffered result exactly.
        for cut in 1..data.len() {
            let chunks = chunk_at(&data, &[cut]);
            let stream =
                futures::stream::iter(chunks.into_iter().map(Ok::<Vec<u8>, anyhow::Error>));
            let (m, t, e) = parse_grpc_web_stream(Box::pin(stream), None).await.unwrap();
            assert_eq!(m, want_m, "messages differ at cut {cut}");
            assert_eq!(t, want_t, "trailers differ at cut {cut}");
            assert_eq!(e.is_some(), want_e.is_some(), "error differs at cut {cut}");
        }
    }

    #[tokio::test]
    async fn connect_stream_parse_matches_buffered_across_chunk_boundaries() {
        // Two data frames + a Connect end-of-stream frame carrying metadata.
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
            let (m, t, _e) = parse_connect_stream(Box::pin(stream), None, &headers)
                .await
                .unwrap();
            assert_eq!(m, want_m, "messages differ at cut {cut}");
            assert_eq!(t, want_t, "trailers differ at cut {cut}");
        }
    }

    #[tokio::test]
    async fn grpc_web_stream_parse_many_tiny_chunks() {
        // A single frame delivered one byte at a time still decodes.
        let body = serde_json::to_vec(&json!({"reply": "hi"})).unwrap();
        let mut data = vec![0x00];
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&body);

        let chunks: Vec<Vec<u8>> = data.iter().map(|b| vec![*b]).collect();
        let stream = futures::stream::iter(chunks.into_iter().map(Ok::<Vec<u8>, anyhow::Error>));
        let (m, _t, _e) = parse_grpc_web_stream(Box::pin(stream), None).await.unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0]["reply"], "hi");
    }

    // TASK 2 — Connect request compression (gzip).

    #[test]
    fn connect_request_envelope_gzip_roundtrips_and_sets_flag() {
        let original = b"a connect request payload long enough to be worth compressing";
        let framed = encode_connect_envelope_compressed(original, false, true).unwrap();

        // Compressed flag (bit 0) set, end-stream (bit 1) clear.
        assert_eq!(framed[0] & 0x01, 0x01, "compressed flag set");
        assert_eq!(framed[0] & 0x02, 0x00, "not end-of-stream");

        // Deframe: length prefix matches the compressed payload, which gunzips
        // back to the original.
        let len = u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]) as usize;
        let payload = &framed[5..5 + len];
        assert_eq!(gzip_decompress(payload).unwrap(), original);

        // Uncompressed default leaves the payload verbatim and the flag clear.
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
        // gRPC-Web message compression.
        assert_eq!(
            compression_header("application/grpc-web+proto", &gz),
            Some(("grpc-encoding", "gzip"))
        );
        // Connect streaming: per-envelope compression.
        assert_eq!(
            compression_header("application/connect+json", &gz),
            Some(("connect-content-encoding", "gzip"))
        );
        // Connect unary: whole-body HTTP content encoding.
        assert_eq!(
            compression_header("application/proto", &gz),
            Some(("content-encoding", "gzip"))
        );
        assert_eq!(
            compression_header("application/json", &gz),
            Some(("content-encoding", "gzip"))
        );
        // Default (uncompressed) advertises nothing.
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
        // Feed a partial header — nothing complete yet.
        dec.extend(&[0x00, 0x00, 0x00]);
        assert!(dec.next_frame().is_none());
        // Complete the header + payload across two more pushes.
        dec.extend(&[0x00, 0x02, 0xAA]);
        assert!(dec.next_frame().is_none(), "payload still short one byte");
        dec.extend(&[0xBB]);
        assert_eq!(dec.next_frame(), Some((0x00, vec![0xAA, 0xBB])));
        assert!(dec.next_frame().is_none());
        assert!(dec.remaining().is_empty());
    }
}
