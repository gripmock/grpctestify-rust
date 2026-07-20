#![allow(clippy::collapsible_if)]
use super::channel::create_channel;
use crate::config::GrpcClientConfig;
use anyhow::{Context, Result, anyhow};

/// Create a fresh pool with well-known types (google.protobuf.*).
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
            return Ok(pool.clone());
        }
    }
    let _guard = DESCRIPTOR_LOAD_MUTEX.lock().await;
    {
        let cache = DESCRIPTOR_CACHE.read().await;
        if let Some(pool) = cache.get(&cache_key) {
            return Ok(pool.clone());
        }
    }
    let pool = match &config.proto_config {
        Some(cfg) if cfg.descriptor.is_some() => {
            load_from_descriptor_file(cfg.descriptor.as_ref().unwrap())?
        }
        Some(cfg) if !cfg.files.is_empty() => load_from_proto_files(&cfg.files, &cfg.import_paths)?,
        _ => load_via_reflection(config).await?,
    };
    let pool_arc = Arc::new(pool);
    DESCRIPTOR_CACHE
        .write()
        .await
        .insert(cache_key, pool_arc.clone());
    Ok(pool_arc)
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

fn load_from_proto_files(files: &[String], import_paths: &[String]) -> Result<DescriptorPool> {
    let fds = protox::compile(files, import_paths)
        .map_err(|e| anyhow!("Failed to compile proto files: {}", e))?;
    let mut pool = new_pool_with_wkt();
    pool.add_file_descriptor_set(fds)
        .map_err(|e| anyhow!("Failed to build pool from proto files: {}", e))?;
    Ok(pool)
}

async fn load_via_reflection(config: &GrpcClientConfig) -> Result<DescriptorPool> {
    let channel = create_channel(config).await?;
    let mut client = ServerReflectionClient::new(channel);
    let mut services = Vec::new();
    let mut files_to_process = Vec::new();

    if let Some(target) = &config.target_service {
        files_to_process.push(target.clone());
    } else {
        let req = ServerReflectionRequest {
            host: config.address.clone(),
            message_request: Some(MessageRequest::ListServices("".to_string())),
        };
        let mut stream = client
            .server_reflection_info(Request::new(futures::stream::iter(vec![req])))
            .await?
            .into_inner();
        if let Some(Ok(msg)) = stream.next().await
            && let Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(resp)) = msg.message_response {
                services = resp.service;
        }
        for s in services {
            if s.name != "grpc.reflection.v1alpha.ServerReflection"
                && s.name != "grpc.reflection.v1.ServerReflection"
            {
                files_to_process.push(s.name);
            }
        }
    }

    let mut fd_bytes = HashMap::new();
    let mut processed = HashSet::new();
    while let Some(sym) = files_to_process.pop() {
        if processed.contains(&sym) {
            continue;
        }
        let req = if sym.ends_with(".proto") {
            ServerReflectionRequest {
                host: config.address.clone(),
                message_request: Some(MessageRequest::FileByFilename(sym.clone())),
            }
        } else {
            ServerReflectionRequest {
                host: config.address.clone(),
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

    let set = prost_types::FileDescriptorSet { file: files };
    if set.file.is_empty() {
        return Err(anyhow!("No descriptors loaded via reflection"));
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
mod tests {
    use super::*;

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

        // WKT can be deserialized from JSON with @type reference
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
