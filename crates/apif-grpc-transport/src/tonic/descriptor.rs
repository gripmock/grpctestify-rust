#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::collapsible_if)]
use super::channel::create_channel;
use crate::config::GrpcClientConfig;
use anyhow::{Context, Result, anyhow};

fn new_pool_with_wkt() -> DescriptorPool {
    prost_types::FileDescriptorSet::default()
        .descriptor()
        .parent_pool()
        .clone()
}
use futures::StreamExt;
use prost::Message;
use prost_reflect::{DescriptorPool, ReflectMessage};
use prost_types::FileDescriptorProto;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tonic::Request;
use tonic_reflection::pb::v1::ServerReflectionRequest;
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;

static DESCRIPTOR_CACHE: LazyLock<RwLock<HashMap<String, Arc<DescriptorPool>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

static DESCRIPTOR_LOAD_MUTEX: LazyLock<TokioMutex<()>> = LazyLock::new(|| TokioMutex::new(()));

pub async fn load_descriptors(config: &GrpcClientConfig) -> Result<Arc<DescriptorPool>> {
    let cache_key = build_cache_key(config);
    {
        let cache = DESCRIPTOR_CACHE.read().await;
        if let Some(pool) = cache.get(&cache_key) {
            tracing::debug!("descriptors: reusing cached pool for {}", config.address);
            return Ok(pool.clone());
        }
    }
    let _guard = DESCRIPTOR_LOAD_MUTEX.lock().await;
    {
        let cache = DESCRIPTOR_CACHE.read().await;
        if let Some(pool) = cache.get(&cache_key) {
            tracing::debug!("descriptors: reusing cached pool for {}", config.address);
            return Ok(pool.clone());
        }
    }
    let pool = match &config.proto_config {
        Some(cfg) if cfg.descriptor.is_some() => {
            let path = cfg.descriptor.as_ref().unwrap();
            tracing::debug!("descriptors: loading from descriptor file {path}");
            load_from_descriptor_file(path)?
        }
        Some(cfg) if !cfg.files.is_empty() => {
            tracing::debug!("descriptors: loading from proto files {:?}", cfg.files);
            load_from_proto_files(&cfg.files, &cfg.import_paths)?
        }
        Some(cfg) => {
            anyhow::bail!(
                "PROTO section names neither `files` nor `descriptor` (import_paths: {:?}); \
                 refusing to fall back to server reflection, which would silently ignore it",
                cfg.import_paths
            );
        }
        None => {
            tracing::debug!(
                "descriptors: loading via reflection from {}",
                config.address
            );
            load_via_reflection(config).await?
        }
    };
    let pool_arc = Arc::new(pool);
    DESCRIPTOR_CACHE
        .write()
        .await
        .insert(cache_key, pool_arc.clone());
    Ok(pool_arc)
}

pub async fn clear_descriptor_cache() {
    DESCRIPTOR_CACHE.write().await.clear();
}

fn build_cache_key(config: &GrpcClientConfig) -> String {
    match &config.proto_config {
        Some(cfg) if cfg.descriptor.is_some() => {
            let d = cfg.descriptor.as_ref().unwrap();
            match &config.target_service {
                Some(t) => format!("descriptor:{}::{}", d, t),
                None => format!("descriptor:{}", d),
            }
        }
        Some(cfg) if !cfg.files.is_empty() => {
            let fk = cfg.files.join(",");
            let ik = cfg.import_paths.join(",");
            match &config.target_service {
                Some(t) => format!("proto:{}:{}::{}", fk, ik, t),
                None => format!("proto:{}:{}", fk, ik),
            }
        }
        _ => match &config.target_service {
            Some(t) => format!("{}::{}", config.address, t),
            None => config.address.clone(),
        },
    }
}

fn load_from_descriptor_file(path: &str) -> Result<DescriptorPool> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read descriptor file: {}", path))?;
    let set = prost_types::FileDescriptorSet::decode(bytes.as_slice())
        .with_context(|| format!("Failed to decode descriptor set: {}", path))?;
    if set.file.is_empty() {
        return Err(anyhow!("Descriptor file contains no descriptors: {}", path));
    }
    let mut pool = new_pool_with_wkt();
    pool.add_file_descriptor_set(set)
        .map_err(|_| anyhow!("Failed to create pool from descriptor file: {}", path))?;
    Ok(pool)
}

fn implied_import_paths(files: &[String]) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for file in files {
        let dir = std::path::Path::new(file)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let dir = if dir.is_empty() { ".".to_string() } else { dir };
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    }
    roots
}

pub(crate) fn load_from_proto_files(
    files: &[String],
    import_paths: &[String],
) -> Result<DescriptorPool> {
    let assumed = implied_import_paths(files);
    let roots: &[String] = if import_paths.is_empty() {
        &assumed
    } else {
        import_paths
    };

    let fds = protox::compile(files, roots).map_err(|e| {
        if import_paths.is_empty() {
            anyhow!(
                "Failed to compile proto files: {} (looked in {}; name the root with `import_paths:` in the PROTO section)",
                e,
                roots.join(", ")
            )
        } else {
            anyhow!("Failed to compile proto files: {}", e)
        }
    })?;
    let mut pool = new_pool_with_wkt();
    pool.add_file_descriptor_set(fds)
        .map_err(|e| anyhow!("Failed to build pool from proto files: {}", e))?;
    Ok(pool)
}

fn reflection_failure(address: &str, status: &tonic::Status) -> anyhow::Error {
    let detail = {
        let mut source: Option<&(dyn std::error::Error + 'static)> =
            std::error::Error::source(status);
        let mut last = status.message().to_string();
        while let Some(err) = source {
            last = err.to_string();
            source = err.source();
        }
        last
    };

    match status.code() {
        tonic::Code::Unimplemented => anyhow!(
            "The server at {address} does not serve reflection. Start it with the reflection \
             service, or name `PROTO descriptor:` (or `PROTO files:`) in the file"
        ),
        tonic::Code::Unavailable | tonic::Code::Unknown => {
            anyhow!("Could not reach {address}: {detail}")
        }
        code => anyhow!("Reflection failed at {address}: {code:?} {detail}"),
    }
}

async fn list_services(
    client: &mut ServerReflectionClient<tonic::transport::Channel>,
    host: &str,
) -> Result<Vec<String>, tonic::Status> {
    let req = ServerReflectionRequest {
        host: host.to_string(),
        message_request: Some(MessageRequest::ListServices("".to_string())),
    };
    let stream = client
        .server_reflection_info(Request::new(futures::stream::iter(vec![req])))
        .await?;
    let mut stream = stream.into_inner();
    let mut out = Vec::new();
    match stream.next().await {
        Some(Ok(msg)) => {
            if let Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(resp)) = msg.message_response {
                for s in resp.service {
                    if s.name != "grpc.reflection.v1alpha.ServerReflection"
                        && s.name != "grpc.reflection.v1.ServerReflection"
                    {
                        out.push(s.name);
                    }
                }
            }
        }
        Some(Err(status)) => return Err(status),
        None => {}
    }
    Ok(out)
}

async fn fetch_descriptors(
    client: &mut ServerReflectionClient<tonic::transport::Channel>,
    host: &str,
    seed: Vec<String>,
) -> HashMap<String, FileDescriptorProto> {
    let mut files_to_process = seed;
    let mut fd_bytes = HashMap::new();
    let mut processed = HashSet::new();
    while let Some(sym) = files_to_process.pop() {
        if processed.contains(&sym) {
            continue;
        }
        let req = if sym.ends_with(".proto") {
            ServerReflectionRequest {
                host: host.to_string(),
                message_request: Some(MessageRequest::FileByFilename(sym.clone())),
            }
        } else {
            ServerReflectionRequest {
                host: host.to_string(),
                message_request: Some(MessageRequest::FileContainingSymbol(sym.clone())),
            }
        };
        let mut stream = match client
            .server_reflection_info(Request::new(futures::stream::iter(vec![req])))
            .await
        {
            Ok(s) => s.into_inner(),
            Err(_) => continue,
        };
        if let Some(Ok(msg)) = stream.next().await
            && let Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::FileDescriptorResponse(resp)) = msg.message_response {
                for b in resp.file_descriptor_proto {
                    if let Ok(fd) = FileDescriptorProto::decode(b.as_slice()) {
                        if let Some(name) = &fd.name {
                            if processed.insert(name.clone()) {
                                let deps = fd.dependency.clone();
                                fd_bytes.insert(name.clone(), fd);
                                for dep in &deps { if !processed.contains(dep) { files_to_process.push(dep.clone()); } }
                            }
                        }
                    }
            }
        }
    }
    fd_bytes
}

async fn load_via_reflection(config: &GrpcClientConfig) -> Result<DescriptorPool> {
    let channel = create_channel(config).await?;
    let mut client = ServerReflectionClient::new(channel);
    let host = config.address.clone();

    let seed = if let Some(target) = &config.target_service {
        vec![target.clone()]
    } else {
        list_services(&mut client, &host)
            .await
            .map_err(|status| reflection_failure(&config.address, &status))?
    };
    tracing::trace!("reflection: seed services {:?}", seed);

    let mut fd_bytes = fetch_descriptors(&mut client, &host, seed).await;

    if fd_bytes.is_empty() && config.target_service.is_some() {
        let all = list_services(&mut client, &host)
            .await
            .map_err(|status| reflection_failure(&config.address, &status))?;
        tracing::debug!(
            "reflection: targeted fetch for {:?} was empty, falling back to full service list {:?}",
            config.target_service,
            all
        );
        if !all.is_empty() {
            fd_bytes = fetch_descriptors(&mut client, &host, all).await;
        }
    }

    let mut files: Vec<_> = fd_bytes.into_values().collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    for f in &mut files {
        f.source_code_info = None;
        if let Some(syn) = &f.syntax {
            if syn == "editions" {
                f.syntax = Some("proto3".to_string());
            }
        }
    }

    tracing::debug!("reflection: fetched {} file descriptor(s)", files.len());
    let set = prost_types::FileDescriptorSet { file: files };
    if set.file.is_empty() {
        return Err(anyhow!(
            "The server at {} answered reflection with no files. Name `PROTO descriptor:` (or \
             `PROTO files:`) in the file to work without reflection",
            config.address
        ));
    }
    match std::panic::catch_unwind(|| {
        let mut pool = new_pool_with_wkt();
        pool.add_file_descriptor_set(set)?;
        Ok::<DescriptorPool, prost_reflect::DescriptorError>(pool)
    }) {
        Ok(Ok(pool)) => Ok(pool),
        _ => Err(anyhow!(
            "Failed to build descriptor pool from reflected descriptors"
        )),
    }
}

#[cfg(test)]
mod reflection_failure_tests {
    use super::reflection_failure;

    #[test]
    fn a_server_without_reflection_is_told_from_one_that_cannot_be_reached() {
        let unimplemented = reflection_failure(
            "localhost:4770",
            &tonic::Status::new(tonic::Code::Unimplemented, "unknown service"),
        )
        .to_string();
        assert!(
            unimplemented.contains("does not serve reflection"),
            "{unimplemented}"
        );
        assert!(
            unimplemented.contains("PROTO descriptor"),
            "{unimplemented}"
        );

        let unreachable = reflection_failure(
            "localhost:59999",
            &tonic::Status::from_error(Box::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "Connection refused (os error 61)",
            ))),
        )
        .to_string();
        assert!(
            unreachable.starts_with("Could not reach localhost:59999"),
            "{unreachable}"
        );
        assert!(unreachable.contains("Connection refused"), "{unreachable}");
    }

    #[test]
    fn any_other_code_keeps_its_own_words() {
        let other = reflection_failure(
            "localhost:4770",
            &tonic::Status::new(tonic::Code::PermissionDenied, "no"),
        )
        .to_string();
        assert!(other.contains("PermissionDenied"), "{other}");
        assert!(other.contains("localhost:4770"), "{other}");
    }
}

#[cfg(test)]
mod proto_source_tests {
    use crate::config::{GrpcClientConfig, ProtoConfig};

    fn config_with(proto: Option<ProtoConfig>) -> GrpcClientConfig {
        GrpcClientConfig {
            address: "127.0.0.1:1".to_string(),
            proto_config: proto,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_proto_section_without_a_source_is_an_error_not_reflection() {
        let config = config_with(Some(ProtoConfig {
            files: Vec::new(),
            import_paths: vec!["proto".to_string()],
            descriptor: None,
        }));
        let err = super::load_descriptors(&config)
            .await
            .expect_err("must not reach reflection");
        let msg = err.to_string();
        assert!(msg.contains("PROTO"), "{msg}");
        assert!(msg.contains("reflection"), "{msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proto_file_named_alone_compiles_from_its_own_folder() {
        let dir = std::env::temp_dir().join(format!("gctf-implied-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.proto");
        std::fs::write(
            &file,
            "syntax = \"proto3\";\npackage hello;\nmessage Ping { string id = 1; }\n",
        )
        .unwrap();

        let pool = load_from_proto_files(&[file.to_string_lossy().to_string()], &[])
            .expect("a file names its own root");
        assert!(pool.get_message_by_name("hello.Ping").is_some());

        assert_eq!(
            implied_import_paths(&[
                "a/one.proto".into(),
                "a/two.proto".into(),
                "b/three.proto".into()
            ]),
            vec!["a".to_string(), "b".to_string()],
            "one root per folder, in the order the files name them",
        );
        assert_eq!(
            implied_import_paths(&["bare.proto".into()]),
            vec![".".to_string()]
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn clearing_forgets_a_pool_a_restarted_server_no_longer_matches() {
        let pool = Arc::new(new_pool_with_wkt());
        DESCRIPTOR_CACHE
            .write()
            .await
            .insert("reflection:localhost:4770".to_string(), pool);
        assert!(!DESCRIPTOR_CACHE.read().await.is_empty());

        clear_descriptor_cache().await;

        assert!(
            DESCRIPTOR_CACHE.read().await.is_empty(),
            "a workbench that outlives the target must be able to forget its schema"
        );
    }

    #[test]
    fn pool_includes_well_known_types() {
        let pool = new_pool_with_wkt();

        assert!(
            pool.get_message_by_name("google.protobuf.StringValue")
                .is_some(),
            "StringValue should be in pool"
        );
        assert!(
            pool.get_message_by_name("google.protobuf.Timestamp")
                .is_some(),
            "Timestamp should be in pool"
        );
        assert!(
            pool.get_message_by_name("google.protobuf.Duration")
                .is_some(),
            "Duration should be in pool"
        );
        assert!(
            pool.get_message_by_name("google.protobuf.Any").is_some(),
            "Any should be in pool"
        );

        let any_desc = pool.get_message_by_name("google.protobuf.Any").unwrap();
        let json =
            r#"{"@type": "type.googleapis.com/google.protobuf.StringValue", "value": "test"}"#;
        let msg = prost_reflect::DynamicMessage::deserialize(
            any_desc.clone(),
            &mut serde_json::Deserializer::from_str(json),
        );
        assert!(
            msg.is_ok(),
            "Any with @type should deserialize: {:?}",
            msg.err()
        );
    }
}
