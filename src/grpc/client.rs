use anyhow::{anyhow, Context, Result};
use futures::stream::{Stream, StreamExt};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor, SerializeOptions};
use prost_types::FileDescriptorProto;

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, Uri};
use tonic::codec::CompressionEncoding;
use tonic::{Request, Status};
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::ServerReflectionRequest;

pub mod codec;
use self::codec::DynamicCodec;

// Global cache for descriptors to avoid race conditions in parallel tests
lazy_static::lazy_static! {
    static ref DESCRIPTOR_CACHE: Mutex<HashMap<String, Arc<DescriptorPool>>> = Mutex::new(HashMap::new());
    static ref CHANNEL_CACHE: Mutex<HashMap<ChannelCacheKey, Channel>> = Mutex::new(HashMap::new());
    static ref DESCRIPTOR_LOAD_MUTEX: TokioMutex<()> = TokioMutex::new(());
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChannelCacheKey {
    address: String,
    timeout_seconds: u64,
    tls_config: Option<TlsConfig>,
    user_agent: String,
}

// Configuration for gRPC client
#[derive(Debug, Clone)]
pub struct GrpcClientConfig {
    pub address: String,
    pub timeout_seconds: u64,
    pub tls_config: Option<TlsConfig>,
    pub proto_config: Option<ProtoConfig>,
    pub metadata: Option<HashMap<String, String>>,
    pub target_service: Option<String>,
    pub compression: CompressionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMode {
    None,
    Gzip,
}

impl Default for CompressionMode {
    fn default() -> Self {
        Self::None
    }
}

impl CompressionMode {
    pub fn from_env() -> Self {
        match std::env::var("GRPCTESTIFY_COMPRESSION")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "gzip" => Self::Gzip,
            _ => Self::None,
        }
    }
}

// Proto configuration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProtoConfig {
    pub files: Vec<String>,
    pub import_paths: Vec<String>,
    pub descriptor: Option<String>,
}

// TLS configuration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TlsConfig {
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
    pub server_name: Option<String>,
    pub insecure_skip_verify: bool,
}

/// gRPC client
pub struct GrpcClient {
    client: tonic::client::Grpc<Channel>,
    descriptor_pool: Arc<DescriptorPool>,
    config: GrpcClientConfig,
}

impl GrpcClient {
    /// Create a new gRPC client
    pub async fn new(config: GrpcClientConfig) -> Result<Self> {
        // Create channel first (lazy)
        let channel = create_channel(&config).await?;
        
        // Load descriptors (might require connection if reflection is used)
        let descriptor_pool = load_descriptors(&config).await?;

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

    pub fn descriptor_pool(&self) -> &DescriptorPool {
        &self.descriptor_pool
    }

    /// Execute a gRPC call with streaming support
    pub async fn call_stream(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: impl Stream<Item = Value> + Send + 'static,
    ) -> Result<(
        HashMap<String, String>,
        Pin<Box<dyn Stream<Item = Result<StreamItem, Status>> + Send + 'static>>,
    )> {
        // Find service and method descriptors
        tracing::debug!("Looking for service descriptor: {}", service_name);
        let service = self
            .descriptor_pool
            .get_service_by_name(service_name)
            .ok_or_else(|| anyhow!("Service '{}' not found in loaded descriptors", service_name))?;

        tracing::debug!("Looking for method descriptor: {}", method_name);
        let method = service
            .methods()
            .find(|m| m.name() == method_name)
            .ok_or_else(|| {
                anyhow!(
                    "Method '{}' not found in service '{}'",
                    method_name,
                    service_name
                )
            })?;

        let input_desc = method.input();

        // Construct path for generic client
        let path_str = format!("/{}/{}", service_name, method_name);
        let path_uri: Uri = path_str.parse().unwrap();
        let path = path_uri
            .path_and_query()
            .ok_or_else(|| anyhow!("Invalid path"))?
            .clone();

        // Prepare request stream
        // Map Value to DynamicMessage, filter errors for now to match Codec::Encode = DynamicMessage
        let request_stream = requests.filter_map(move |json| {
            let input_desc = input_desc.clone();
            async move {
                let mut normalized = json;
                normalize_input_json_for_well_known(&mut normalized, &input_desc);

                let json_str = match serde_json::to_string(&normalized) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("JSON serialization error: {}", e);
                        return None;
                    }
                };

                let mut deserializer = serde_json::Deserializer::from_str(&json_str);
                match prost_reflect::DynamicMessage::deserialize(input_desc, &mut deserializer) {
                    Ok(msg) => Some(msg),
                    Err(e) => {
                        tracing::error!("Protobuf deserialization error: {}", e);
                        None
                    }
                }
            }
        });

        // Pin the stream so we can iterate it
        let mut request_stream = Box::pin(request_stream);

        // Create dynamic codec
        let codec = DynamicCodec::new(method.input(), method.output());

        // Clone client to ensure we have a fresh handle that we can make ready and use
        let mut client = self.client.clone();
        
        // Ensure client is ready
        // We ignore the error here because `ready()` returns a reference to the service,
        // but `unary` consumes `self`. We just need to drive the readiness check.
        // If it fails, the subsequent call will likely fail too.
        tracing::debug!("Checking client readiness...");
        if let Err(e) = client.ready().await {
             tracing::warn!("Client readiness check failed: {}", e);
        }
        tracing::debug!("Client ready");

        let response_stream: Pin<Box<dyn Stream<Item = Result<StreamItem, Status>> + Send>> =
            if method.is_client_streaming() && method.is_server_streaming() {
                // Bidi Streaming
                let mut request = Request::new(request_stream);
                let meta = request.metadata_mut();
                insert_request_metadata(meta, self.config.metadata.as_ref());

                let response = client.streaming(request, path, codec).await?;
                let inner = response.into_inner();
                Box::pin(inner.map(|item| item.map(|msg| StreamItem::Message(dynamic_message_to_json(&msg)))))
            } else if method.is_server_streaming() {
                // Server Streaming (Client Unary)
                // We need exactly one request
                let first_msg = request_stream
                    .next()
                    .await
                    .ok_or_else(|| Status::invalid_argument("Missing request message"))?;

                let mut request = Request::new(first_msg);
                let meta = request.metadata_mut();
                insert_request_metadata(meta, self.config.metadata.as_ref());

                let response = client.server_streaming(request, path, codec).await?;
                let inner = response.into_inner();
                Box::pin(inner.map(|item| item.map(|msg| StreamItem::Message(dynamic_message_to_json(&msg)))))
            } else if method.is_client_streaming() {
                // Client Streaming (Server Unary)
                let mut request = Request::new(request_stream);
                let meta = request.metadata_mut();
                insert_request_metadata(meta, self.config.metadata.as_ref());

                let response = client.client_streaming(request, path, codec).await?;
                let msg = response.into_inner();
                let val = dynamic_message_to_json(&msg);
                Box::pin(futures::stream::once(async move {
                    Ok(StreamItem::Message(val))
                }))
            } else {
                // Unary
                let first_msg = request_stream
                    .next()
                    .await
                    .ok_or_else(|| Status::invalid_argument("Missing request message"))?;
                let mut request = Request::new(first_msg);
                let meta = request.metadata_mut();
                insert_request_metadata(meta, self.config.metadata.as_ref());

                let response = client.unary(request, path, codec).await?;
                let msg = response.into_inner();
                let val = dynamic_message_to_json(&msg);
                Box::pin(futures::stream::once(async move {
                    Ok(StreamItem::Message(val))
                }))
            };

        let headers = HashMap::new();

        Ok((headers, response_stream))
    }

    /// Describe service/method using reflection
    pub fn describe(&self, symbol: Option<&str>) -> Result<String> {
        if let Some(sym) = symbol {
            // Parse symbol (format: package.Service/Method)
            let parts: Vec<&str> = sym.split('/').collect();
            if parts.len() != 2 {
                // Try finding it as a service first
                if let Some(service) = self.descriptor_pool.get_service_by_name(sym) {
                    let mut output = format!("Service: {}\n", service.name());
                    for method in service.methods() {
                        let input_type = method.input().name().to_string();
                        let output_type = method.output().name().to_string();
                        output.push_str(&format!(
                            "  rpc {}({}) returns ({});\n",
                            method.name(),
                            input_type,
                            output_type
                        ));
                    }
                    return Ok(output);
                }
                return Ok(format!("Invalid symbol format: {}. Expected 'package.Service/Method' or 'package.Service'", sym));
            }

            let service_name = parts[0];
            let method_name = parts[1];

            // Find service
            let service = self
                .descriptor_pool
                .get_service_by_name(service_name)
                .ok_or_else(|| anyhow!("Service '{}' not found", service_name))?;

            // Find method
            let method = service
                .methods()
                .find(|m| m.name() == method_name)
                .ok_or_else(|| {
                    anyhow!(
                        "Method '{}' not found in service '{}'",
                        method_name,
                        service_name
                    )
                })?;

            let input_desc = method.input();
            let output_desc = method.output();

            Ok(format!(
                "rpc {}({}) returns ({})\n  Input: {}\n  Output: {}",
                method_name,
                input_desc.name(),
                output_desc.name(),
                input_desc.full_name(),
                output_desc.full_name()
            ))
        } else {
            // List all services
            let services: Vec<_> = self
                .descriptor_pool
                .services()
                .map(|s| s.name().to_string())
                .collect();

            Ok(format!(
                "Services ({}):\n  - {}",
                services.len(),
                services.join("\n  - ")
            ))
        }
    }
    pub async fn call(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: Vec<Value>,
    ) -> Result<TestResponse> {
        let stream = futures::stream::iter(requests);
        let (headers, mut response_stream) =
            self.call_stream(service_name, method_name, stream).await?;

        let mut messages = Vec::new();
        let mut trailers = HashMap::new();

        while let Some(item_res) = response_stream.next().await {
            match item_res? {
                StreamItem::Message(msg) => messages.push(msg),
                StreamItem::Trailers(t) => trailers.extend(t),
            }
        }

        Ok(TestResponse {
            headers,
            messages,
            trailers,
        })
    }
}

fn user_agent_value() -> String {
    format!("grpctestify/{}", env!("CARGO_PKG_VERSION"))
}

fn resolve_user_agent(custom_metadata: Option<&HashMap<String, String>>) -> String {
    if let Some(metadata) = custom_metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                return v.clone();
            }
        }
    }

    user_agent_value()
}

fn insert_request_metadata(
    meta: &mut MetadataMap,
    custom_metadata: Option<&HashMap<String, String>>,
) {
    if let Some(metadata) = custom_metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            let normalized_key = k.to_ascii_lowercase();
            if let Ok(key) = MetadataKey::from_str(&normalized_key) {
                if let Ok(val) = MetadataValue::from_str(v) {
                    meta.insert(key, val);
                }
            }
        }
    }
}

fn normalize_input_json_for_well_known(value: &mut Value, desc: &MessageDescriptor) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };

    let keys: Vec<String> = obj.keys().cloned().collect();
    for key in keys {
        let Some(field) = desc
            .get_field_by_json_name(&key)
            .or_else(|| desc.get_field_by_name(&key))
        else {
            continue;
        };

        let Some(field_value) = obj.get_mut(&key) else {
            continue;
        };

        if let Kind::Message(message_desc) = field.kind() {
            if message_desc.full_name() == "google.protobuf.FieldMask" {
                if let Some(paths) = field_value
                    .as_object()
                    .and_then(|m| m.get("paths"))
                    .and_then(|v| v.as_array())
                {
                    let joined = paths
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(",");
                    *field_value = Value::String(joined);
                    continue;
                }
            }

            if field.is_list() {
                if let Some(arr) = field_value.as_array_mut() {
                    for item in arr {
                        normalize_input_json_for_well_known(item, &message_desc);
                    }
                }
            } else {
                normalize_input_json_for_well_known(field_value, &message_desc);
            }
        }
    }
}

fn dynamic_message_to_json(msg: &DynamicMessage) -> Value {
    let options = SerializeOptions::new().use_proto_field_name(true);
    msg.serialize_with_options(serde_json::value::Serializer, &options)
        .unwrap_or(Value::Null)
}

#[derive(Debug, Clone)]
pub struct TestResponse {
    pub headers: HashMap<String, String>,
    pub messages: Vec<Value>,
    pub trailers: HashMap<String, String>,
}

/// Stream item types
#[derive(Debug)]
pub enum StreamItem {
    Message(serde_json::Value),
    Trailers(HashMap<String, String>),
}

// Helper function to create channel
async fn create_channel(config: &GrpcClientConfig) -> Result<Channel> {
    let user_agent = resolve_user_agent(config.metadata.as_ref());

    let cache_key = ChannelCacheKey {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        user_agent: user_agent.clone(),
    };

    {
        let cache = CHANNEL_CACHE.lock().unwrap();
        if let Some(channel) = cache.get(&cache_key) {
            tracing::debug!("Cache hit for channel to {}", config.address);
            return Ok(channel.clone());
        }
    }

    tracing::debug!(
        "Cache miss for channel to {}, creating new connection...",
        config.address
    );

    let channel = if let Some(tls_config) = &config.tls_config {
        let mut tls = ClientTlsConfig::new();

        if let Some(domain) = &tls_config.server_name {
            tls = tls.domain_name(domain);
        }

        if let Some(ca_path) = &tls_config.ca_cert_path {
            let ca_pem =
                std::fs::read_to_string(ca_path).context("Failed to read CA certificate")?;
            tls = tls.ca_certificate(Certificate::from_pem(ca_pem));
        }

        if let (Some(cert_path), Some(key_path)) =
            (&tls_config.client_cert_path, &tls_config.client_key_path)
        {
            let cert_pem =
                std::fs::read_to_string(cert_path).context("Failed to read client certificate")?;
            let key_pem = std::fs::read_to_string(key_path).context("Failed to read client key")?;
            tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
        }

        // Ensure scheme is present for TLS (https://)
        let addr = if !config.address.contains("://") {
            format!("https://{}", config.address)
        } else {
            config.address.clone()
        };

        let endpoint = Channel::from_shared(addr)
            .context("Invalid address format")?
            .timeout(Duration::from_secs(config.timeout_seconds))
            .user_agent(user_agent.clone())
            .context("Invalid user-agent value")?;

        endpoint
            .tls_config(tls)
            .context("Failed to configure TLS")?
            .connect_lazy()
    } else {
        // Ensure scheme is present for plaintext (http://)
        let addr = if !config.address.contains("://") {
            format!("http://{}", config.address)
        } else {
            config.address.clone()
        };

        let endpoint = Channel::from_shared(addr)
            .context("Invalid address format")?
            .timeout(Duration::from_secs(config.timeout_seconds))
            .user_agent(user_agent)
            .context("Invalid user-agent value")?;

        endpoint
            .connect_lazy()
    };

    {
        let mut cache = CHANNEL_CACHE.lock().unwrap();
        cache.insert(cache_key, channel.clone());
    }

    Ok(channel)
}

/// Load descriptors with caching
async fn load_descriptors(config: &GrpcClientConfig) -> Result<Arc<DescriptorPool>> {
    // Check if we have proto config
    if let Some(proto_config) = &config.proto_config {
        if !proto_config.files.is_empty() {
            return Err(anyhow!(
                "PROTO files are not supported in native mode. Use PROTO descriptor=<path> or server reflection."
            ));
        }
    }

    let cache_key = if let Some(target) = &config.target_service {
        format!("{}::{}", config.address, target)
    } else {
        config.address.clone()
    };

    {
        let cache = DESCRIPTOR_CACHE.lock().unwrap();
        if let Some(pool) = cache.get(&cache_key) {
            tracing::debug!("Cache hit for descriptors from {}", cache_key);
            return Ok(pool.clone());
        }
    }

    let _load_guard = DESCRIPTOR_LOAD_MUTEX.lock().await;

    {
        let cache = DESCRIPTOR_CACHE.lock().unwrap();
        if let Some(pool) = cache.get(&cache_key) {
            tracing::debug!("Cache hit for descriptors from {}", cache_key);
            return Ok(pool.clone());
        }
    }

    tracing::debug!("Cache miss for descriptors from {}, loading...", cache_key);

    let pool = load_descriptors_via_reflection(config).await?;
    let pool_arc = Arc::new(pool);

    {
        let mut cache = DESCRIPTOR_CACHE.lock().unwrap();
        cache.insert(cache_key, pool_arc.clone());
    }

    Ok(pool_arc)
}

/// Load descriptors via Server Reflection
async fn load_descriptors_via_reflection(config: &GrpcClientConfig) -> Result<DescriptorPool> {
    tracing::info!("Loading descriptors via reflection from {}", config.address);

    let channel = create_channel(config).await?;
    let mut client = ServerReflectionClient::new(channel);

    tracing::debug!("Connected to reflection service, creating stream...");

    let mut services = Vec::new();
    let mut files_to_process = Vec::new();

    if let Some(target) = &config.target_service {
        tracing::debug!("Queueing target service for descriptor fetch: {}", target);
        files_to_process.push(target.clone());
    } else {
        // List all services if no target specified
        let list_services_req = ServerReflectionRequest {
            host: config.address.clone(),
            message_request: Some(MessageRequest::ListServices("".to_string())),
        };

        let request = Request::new(futures::stream::iter(vec![list_services_req]));
        let mut response_stream = client.server_reflection_info(request).await?.into_inner();

        if let Some(response) = response_stream.next().await {
            let msg = response?;
            if let Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(resp)) = msg.message_response {
                services = resp.service;
            }
        }

        // Initial population from services
        for service in services {
            if service.name == "grpc.reflection.v1alpha.ServerReflection"
                || service.name == "grpc.reflection.v1.ServerReflection"
            {
                continue;
            }
            tracing::debug!("Queueing service for descriptor fetch: {}", service.name);
            files_to_process.push(service.name);
        }
    }

    let mut file_descriptors_bytes = HashMap::new();
    let mut processed_files = HashSet::new();

    while let Some(symbol_or_filename) = files_to_process.pop() {
        if processed_files.contains(&symbol_or_filename) {
            continue;
        }

        tracing::debug!("Processing descriptor request for: {}", symbol_or_filename);
        
        // Determine request type based on whether it looks like a filename
        let req = if symbol_or_filename.ends_with(".proto") {
             ServerReflectionRequest {
                host: config.address.clone(),
                message_request: Some(MessageRequest::FileByFilename(symbol_or_filename.clone())),
            }
        } else {
             ServerReflectionRequest {
                host: config.address.clone(),
                message_request: Some(MessageRequest::FileContainingSymbol(symbol_or_filename.clone())),
            }
        };

        let request = Request::new(futures::stream::iter(vec![req]));
        let mut response_stream = match client.server_reflection_info(request).await {
            Ok(s) => s.into_inner(),
            Err(e) => {
                tracing::warn!(
                    "Failed to initiate reflection request for {}: {}",
                    symbol_or_filename,
                    e
                );
                continue;
            }
        };

        if let Some(response_result) = response_stream.next().await {
            let msg = match response_result {
                Ok(m) => m,
                Err(e) => {
                     tracing::warn!("Error receiving descriptor response for {}: {}", symbol_or_filename, e);
                     continue;
                }
            };
            
            if let Some(tonic_reflection::pb::v1::server_reflection_response::MessageResponse::FileDescriptorResponse(resp)) = msg.message_response {
                for fd_bytes in resp.file_descriptor_proto {
                    if let Ok(fd) = FileDescriptorProto::decode(fd_bytes.as_slice()) {
                         if let Some(name) = &fd.name {
                            if !processed_files.contains(name) {
                                tracing::debug!("Loaded descriptor for file: {}", name);
                                processed_files.insert(name.clone());
                                file_descriptors_bytes.insert(name.clone(), fd.clone());
                                
                                // Enqueue dependencies
                                for dep in fd.dependency {
                                    if !processed_files.contains(&dep) {
                                         tracing::debug!("Found dependency: {}", dep);
                                         files_to_process.push(dep);
                                    }
                                }
                            }
                         }
                    }
                }
            }
        }
    }

    // Sort files topologically-ish? Or just by name to be deterministic?
    // prost-reflect should handle unordered, but let's try to be deterministic.
    let mut file_descriptors: Vec<_> = file_descriptors_bytes.into_values().collect();
    file_descriptors.sort_by(|a, b| a.name.cmp(&b.name));

    let mut filtered_files = Vec::new();
    let mut names_seen = HashSet::new();

    for mut f in file_descriptors {
        // Some servers use synthetic names like `service.proto.src`; keep them,
        // they can still contain valid service descriptors.
        
        // Ensure no duplicates in the final list (though hashmap should prevent this)
        if let Some(name) = &f.name {
            if names_seen.contains(name) {
                tracing::warn!("Duplicate file found in final list: {}", name);
                continue;
            }
            names_seen.insert(name.clone());
        }

        // Sanitize public_dependency
        let dep_len = f.dependency.len() as i32;
        let mut valid_public_deps = Vec::new();
        for &dep_idx in &f.public_dependency {
            if dep_idx >= 0 && dep_idx < dep_len {
                valid_public_deps.push(dep_idx);
            } else {
                tracing::warn!(
                    "Sanitizing invalid public_dependency index {} in file {} (dependency count: {})",
                    dep_idx,
                    f.name.as_deref().unwrap_or("<unknown>"),
                    dep_len
                );
            }
        }
        f.public_dependency = valid_public_deps;

        // Sanitize weak_dependency
        let mut valid_weak_deps = Vec::new();
        for &dep_idx in &f.weak_dependency {
            if dep_idx >= 0 && dep_idx < dep_len {
                valid_weak_deps.push(dep_idx);
            } else {
                tracing::warn!(
                    "Sanitizing invalid weak_dependency index {} in file {} (dependency count: {})",
                    dep_idx,
                    f.name.as_deref().unwrap_or("<unknown>"),
                    dep_len
                );
            }
        }
        f.weak_dependency = valid_weak_deps;

        // Sanitize messages (oneof_index)
        for msg in &mut f.message_type {
            let oneof_count = msg.oneof_decl.len() as i32;
            for field in &mut msg.field {
                if let Some(idx) = field.oneof_index {
                    if idx < 0 || idx >= oneof_count {
                        tracing::warn!(
                            "Sanitizing invalid oneof_index {} in message {} in file {} (oneof count: {})",
                            idx,
                            msg.name.as_deref().unwrap_or("<unknown>"),
                            f.name.as_deref().unwrap_or("<unknown>"),
                            oneof_count
                        );
                        field.oneof_index = None;
                    }
                }
            }
        }

        // Sanitize syntax
        if let Some(syntax) = &f.syntax {
            if syntax == "editions" {
                // Compatibility mode: prost-reflect 0.14 may fail on some reflected
                // editions descriptors. Downgrade syntax marker for invocation.
                f.syntax = Some("proto3".to_string());
            } else if syntax != "proto2" && syntax != "proto3" && !syntax.is_empty() {
                tracing::warn!(
                    "Sanitizing unknown syntax '{}' in file {} to 'proto3'",
                    syntax,
                    f.name.as_deref().unwrap_or("<unknown>")
                );
                f.syntax = Some("proto3".to_string());
            }
        }

        // SourceCodeInfo is not required for invocation and may contain invalid paths
        // in reflection payloads from some servers, which can trigger panics in
        // descriptor diagnostic formatting inside prost-reflect.
        f.source_code_info = None;

        filtered_files.push(f);
    }
    
    // Check for missing dependencies
    let available_files: HashSet<String> = filtered_files.iter().filter_map(|f| f.name.clone()).collect();
    for f in &filtered_files {
        for dep in &f.dependency {
            if !available_files.contains(dep) {
                tracing::warn!("File {} depends on missing file {}", f.name.as_deref().unwrap_or("<unknown>"), dep);
            }
        }
    }

    let file_descriptor_set = prost_types::FileDescriptorSet {
        file: filtered_files,
    };

    if file_descriptor_set.file.is_empty() {
        return Err(anyhow!("No descriptors loaded via reflection for target service"));
    }
    
    tracing::debug!("Building descriptor pool with {} files (filtered)", file_descriptor_set.file.len());

    let build_result = std::panic::catch_unwind(|| {
        let mut pool = DescriptorPool::global();
        pool.add_file_descriptor_set(file_descriptor_set)?;
        Ok::<DescriptorPool, prost_reflect::DescriptorError>(pool)
    });

    match build_result {
        Ok(result) => match result {
            Ok(pool) => Ok(pool),
            Err(_) => Err(anyhow!(
                "Failed to create descriptor pool from reflected descriptors"
            )),
        },
        Err(_) => Err(anyhow!(
            "Descriptor pool construction panicked on reflected descriptors"
        )),
    }
}
