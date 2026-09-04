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

type CachedPool = Option<prost_reflect::DescriptorPool>;

static POOL_CACHE: LazyLock<RwLock<HashMap<String, CachedPool>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

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

pub async fn clear_mode_cache() {
    MODE_CACHE.write().await.clear();
    POOL_CACHE.write().await.clear();
}

pub(crate) async fn pool_for(config: &GrpcClientConfig) -> CachedPool {
    let key = format!("{}|{:?}", config.address, config.protocol);
    if let Some(cached) = POOL_CACHE.read().await.get(&key) {
        return cached.clone();
    }
    let pool = load_pool(config).await;
    POOL_CACHE.write().await.insert(key, pool.clone());
    pool
}

async fn ask(config: &GrpcClientConfig, message_request: MessageRequest) -> Option<Vec<u8>> {
    let request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(message_request),
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
    Some(response_bytes)
}

async fn fetch_service_modes(
    config: &GrpcClientConfig,
    service_name: &str,
) -> Option<ServiceModes> {
    let response_bytes = ask(
        config,
        MessageRequest::FileContainingSymbol(service_name.to_string()),
    )
    .await?;

    let modes = collect_modes(&response_bytes, service_name);
    if modes.is_empty() { None } else { Some(modes) }
}

pub(crate) fn services_in(response_bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for payload in crate::grpc::web::data_frame_payloads(response_bytes) {
        let Ok(response) = ServerReflectionResponse::decode(payload.as_slice()) else {
            continue;
        };
        let Some(MessageResponse::ListServicesResponse(list)) = response.message_response else {
            continue;
        };
        for service in list.service {
            if service.name != "grpc.reflection.v1.ServerReflection"
                && service.name != "grpc.reflection.v1alpha.ServerReflection"
            {
                out.push(service.name);
            }
        }
    }
    out
}

pub(crate) fn files_in(response_bytes: &[u8]) -> Vec<prost_types::FileDescriptorProto> {
    let mut out = Vec::new();
    for payload in crate::grpc::web::data_frame_payloads(response_bytes) {
        let Ok(response) = ServerReflectionResponse::decode(payload.as_slice()) else {
            continue;
        };
        let Some(MessageResponse::FileDescriptorResponse(files)) = response.message_response else {
            continue;
        };
        for file in files.file_descriptor_proto {
            if let Ok(file) = prost_types::FileDescriptorProto::decode(file.as_slice()) {
                out.push(file);
            }
        }
    }
    out
}

pub(crate) async fn load_pool(config: &GrpcClientConfig) -> Option<prost_reflect::DescriptorPool> {
    let listed = ask(config, MessageRequest::ListServices(String::new())).await?;
    let seed = services_in(&listed);
    if seed.is_empty() {
        return None;
    }

    let mut pending = seed;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut files: HashMap<String, prost_types::FileDescriptorProto> = HashMap::new();

    while let Some(symbol) = pending.pop() {
        if !seen.insert(symbol.clone()) {
            continue;
        }
        let request = if symbol.ends_with(".proto") {
            MessageRequest::FileByFilename(symbol.clone())
        } else {
            MessageRequest::FileContainingSymbol(symbol.clone())
        };
        let Some(bytes) = ask(config, request).await else {
            continue;
        };
        for mut file in files_in(&bytes) {
            let Some(name) = file.name.clone() else {
                continue;
            };
            if files.contains_key(&name) {
                continue;
            }
            for dependency in &file.dependency {
                if !seen.contains(dependency) {
                    pending.push(dependency.clone());
                }
            }
            file.source_code_info = None;
            if file.syntax.as_deref() == Some("editions") {
                file.syntax = Some("proto3".to_string());
            }
            files.insert(name, file);
        }
    }

    let mut file: Vec<_> = files.into_values().collect();
    file.sort_by(|a, b| a.name.cmp(&b.name));
    prost_reflect::DescriptorPool::from_file_descriptor_set(prost_types::FileDescriptorSet { file })
        .ok()
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
