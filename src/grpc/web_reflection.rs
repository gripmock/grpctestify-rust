//! Server reflection over the HTTP transports.
//!
//! Connect picks the content type and the framing from whether a method
//! streams (`application/connect+{codec}` and envelopes for streaming, the bare
//! `application/{codec}` and a plain body for unary), and a server is entitled
//! to answer 415 when a client gets that wrong. The section shape of a `.gctf`
//! is not enough to tell: a server-streaming method with one `RESPONSE` looks
//! exactly like a unary one. So when no local descriptor is configured, ask the
//! server -- `ServerReflectionInfo` is reachable over the very same port.

use crate::grpc::{GrpcClientConfig, RpcMode, WireProtocol};
use prost::Message;
use std::collections::HashMap;
use std::sync::LazyLock;
use tokio::sync::RwLock;
use tonic_reflection::pb::v1::{
    ServerReflectionRequest, ServerReflectionResponse, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
};

const REFLECTION_SERVICE: &str = "grpc.reflection.v1.ServerReflection";
const REFLECTION_METHOD: &str = "ServerReflectionInfo";

type ServiceModes = HashMap<String, RpcMode>;

static MODE_CACHE: LazyLock<RwLock<HashMap<String, Option<ServiceModes>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Resolve a method's RPC mode from server reflection, `None` when the server
/// serves no reflection or does not know the method. Cached per address and
/// service; a failed lookup is cached too, so an unreflective server costs one
/// round trip per service rather than one per test.
pub(crate) async fn resolve_rpc_mode(
    config: &GrpcClientConfig,
    service_name: &str,
    method_name: &str,
) -> Option<RpcMode> {
    let key = format!("{}|{}", config.address, service_name);

    if let Some(cached) = MODE_CACHE.read().await.get(&key) {
        return cached.as_ref()?.get(method_name).copied();
    }

    let modes = fetch_service_modes(config, service_name).await;
    MODE_CACHE.write().await.insert(key, modes.clone());

    modes?.get(method_name).copied()
}

async fn fetch_service_modes(
    config: &GrpcClientConfig,
    service_name: &str,
) -> Option<ServiceModes> {
    let request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::FileContainingSymbol(
            service_name.to_string(),
        )),
    };

    let (content_type, body) = match config.protocol {
        WireProtocol::GrpcWeb => (
            "application/grpc-web+proto",
            crate::grpc::web::encode_grpc_web_frame(&request.encode_to_vec()),
        ),
        _ => (
            "application/connect+proto",
            crate::grpc::web::encode_connect_frame(&request.encode_to_vec()),
        ),
    };

    let (status, response_bytes, _headers) = crate::grpc::web::send_http_post(
        config,
        REFLECTION_SERVICE,
        REFLECTION_METHOD,
        content_type,
        &body,
    )
    .await
    .ok()?;

    if !status.is_success() {
        return None;
    }

    let modes = collect_modes(&response_bytes, service_name);
    if modes.is_empty() { None } else { Some(modes) }
}

fn collect_modes(response_bytes: &[u8], service_name: &str) -> ServiceModes {
    let mut modes = ServiceModes::new();

    for payload in crate::grpc::web::data_frame_payloads(response_bytes) {
        let Ok(response) = ServerReflectionResponse::decode(payload.as_slice()) else {
            continue;
        };
        let Some(MessageResponse::FileDescriptorResponse(files)) = response.message_response else {
            continue;
        };

        for file in files.file_descriptor_proto {
            let Ok(file) = prost_types::FileDescriptorProto::decode(file.as_slice()) else {
                continue;
            };
            let package = file.package();

            for service in &file.service {
                let full_name = if package.is_empty() {
                    service.name().to_string()
                } else {
                    format!("{}.{}", package, service.name())
                };
                if full_name != service_name {
                    continue;
                }

                for method in &service.method {
                    modes.insert(
                        method.name().to_string(),
                        match (
                            method.client_streaming.unwrap_or(false),
                            method.server_streaming.unwrap_or(false),
                        ) {
                            (false, false) => RpcMode::Unary,
                            (true, false) => RpcMode::ClientStream,
                            (false, true) => RpcMode::ServerStream,
                            (true, true) => RpcMode::Bidi,
                        },
                    );
                }
            }
        }
    }

    modes
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::{FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto};
    use tonic_reflection::pb::v1::FileDescriptorResponse;

    fn method(name: &str, client: bool, server: bool) -> MethodDescriptorProto {
        MethodDescriptorProto {
            name: Some(name.to_string()),
            client_streaming: Some(client),
            server_streaming: Some(server),
            ..Default::default()
        }
    }

    fn response_bytes() -> Vec<u8> {
        let file = FileDescriptorProto {
            name: Some("service.proto".to_string()),
            package: Some("multiverse.v1".to_string()),
            service: vec![ServiceDescriptorProto {
                name: Some("MultiverseService".to_string()),
                method: vec![
                    method("Ping", false, false),
                    method("UploadData", true, false),
                    method("StreamData", false, true),
                    method("Chat", true, true),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let response = ServerReflectionResponse {
            message_response: Some(MessageResponse::FileDescriptorResponse(
                FileDescriptorResponse {
                    file_descriptor_proto: vec![file.encode_to_vec()],
                },
            )),
            ..Default::default()
        };

        crate::grpc::web::encode_connect_frame(&response.encode_to_vec())
    }

    #[test]
    fn collect_modes_reads_every_streaming_shape() {
        let modes = collect_modes(&response_bytes(), "multiverse.v1.MultiverseService");

        assert_eq!(modes.get("Ping"), Some(&RpcMode::Unary));
        assert_eq!(modes.get("UploadData"), Some(&RpcMode::ClientStream));
        assert_eq!(modes.get("StreamData"), Some(&RpcMode::ServerStream));
        assert_eq!(modes.get("Chat"), Some(&RpcMode::Bidi));
    }

    // The response carries every file the symbol needs, so services other than
    // the requested one must not leak into the result.
    #[test]
    fn collect_modes_ignores_other_services() {
        assert!(collect_modes(&response_bytes(), "other.v1.OtherService").is_empty());
    }

    #[test]
    fn collect_modes_skips_the_end_of_stream_frame() {
        let mut body = response_bytes();
        body.extend_from_slice(&crate::grpc::web::encode_connect_frame_end());

        assert_eq!(
            collect_modes(&body, "multiverse.v1.MultiverseService").len(),
            4
        );
    }
}
