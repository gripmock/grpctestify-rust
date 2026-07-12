use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock, RwLock};
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

use crate::grpc::proxy::ProxyEnv;
use crate::grpc::tls::{ChannelCacheKey, GrpcClientConfig, TlsConfig};

static CHANNEL_CACHE: LazyLock<RwLock<HashMap<ChannelCacheKey, Channel>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Ensures proxy warnings are emitted at most once per process lifetime.
static PROXY_WARNED: OnceLock<()> = OnceLock::new();

/// Create a channel to a gRPC server.
///
/// Channels are cached by (address, timeout, tls, user_agent, connection_id).
/// Set `config.connection_id` to a unique value to bypass the cache and create
/// an independent TCP/TLS connection (useful for connection pooling in bench).
pub async fn create_channel(config: &GrpcClientConfig) -> Result<Channel> {
    if config.address.is_empty() {
        return Err(anyhow::anyhow!("gRPC address cannot be empty"));
    }

    if !config.address.contains(':') {
        return Err(anyhow::anyhow!(
            "Invalid gRPC address format '{}'. Expected format: host:port or scheme://host:port",
            config.address
        ));
    }

    let user_agent = resolve_user_agent(config.user_agent.as_deref(), config.metadata.as_ref());

    let mut cache_key = ChannelCacheKey {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        user_agent: user_agent.clone(),
    };

    // connection_id > 0 creates independent cache entries (connection pooling)
    if config.connection_id > 0 {
        cache_key.user_agent = format!("{}/conn-{}", cache_key.user_agent, config.connection_id);
    }

    {
        let cache = CHANNEL_CACHE.read().expect("lock poisoned");
        if let Some(channel) = cache.get(&cache_key) {
            tracing::debug!("Cache hit for channel to {}", config.address);
            return Ok(channel.clone());
        }
    }

    tracing::debug!(
        "Cache miss for channel to {}, creating new connection...",
        config.address
    );

    PROXY_WARNED.get_or_init(|| ProxyEnv::from_env().warn_if_set());
    let channel = if let Some(tls_config) = &config.tls_config {
        create_tls_channel(config, tls_config).await?
    } else {
        create_plaintext_channel(config).await?
    };

    {
        let mut cache = CHANNEL_CACHE.write().expect("lock poisoned");
        cache.insert(cache_key, channel.clone());
    }

    Ok(channel)
}

async fn create_tls_channel(config: &GrpcClientConfig, tls_config: &TlsConfig) -> Result<Channel> {
    let mut tls = ClientTlsConfig::new();

    if let Some(domain) = &tls_config.server_name {
        tls = tls.domain_name(domain);
    }

    if let Some(ca_path) = &tls_config.ca_cert_path {
        let ca_pem = std::fs::read_to_string(ca_path).context("Failed to read CA certificate")?;
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

    if tls_config.insecure_skip_verify {
        tracing::warn!(
            "SECURITY WARNING: TLS certificate verification is disabled (insecure_skip_verify=true). \
            This is insecure and should only be used for testing or development."
        );
    }

    let addr = if !config.address.contains("://") {
        format!("https://{}", config.address)
    } else {
        config.address.clone()
    };

    let endpoint = build_endpoint(addr, config.timeout_seconds)?;

    Ok(endpoint
        .tls_config(tls)
        .context("Failed to configure TLS")?
        .connect_lazy())
}

async fn create_plaintext_channel(config: &GrpcClientConfig) -> Result<Channel> {
    let addr = if !config.address.contains("://") {
        format!("http://{}", config.address)
    } else {
        config.address.clone()
    };

    let endpoint = build_endpoint(addr, config.timeout_seconds)?;

    Ok(endpoint.connect_lazy())
}

fn build_endpoint(addr: String, timeout_secs: u64) -> Result<tonic::transport::Endpoint> {
    Ok(Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(timeout_secs)))
}

fn user_agent_value() -> String {
    format!("grpctestify/{}", env!("CARGO_PKG_VERSION"))
}

fn resolve_user_agent(
    explicit: Option<&str>,
    custom_metadata: Option<&HashMap<String, String>>,
) -> String {
    if let Some(ua) = explicit {
        return ua.to_string();
    }
    if let Some(metadata) = custom_metadata {
        for (k, v) in metadata {
            if k.eq_ignore_ascii_case("user-agent") {
                return v.clone();
            }
        }
    }
    user_agent_value()
}

#[cfg(test)]
fn insert_user_agent(metadata: &mut tonic::metadata::MetadataMap, user_agent: &str) {
    use std::str::FromStr as _;
    if let Ok(val) = tonic::metadata::MetadataValue::from_str(user_agent) {
        metadata.insert("user-agent", val);
    }
}

#[cfg(test)]
fn insert_custom_metadata(
    metadata: &mut tonic::metadata::MetadataMap,
    custom: Option<&HashMap<String, String>>,
) {
    use std::str::FromStr as _;
    if let Some(map) = custom {
        for (key, value) in map {
            if key.to_lowercase() == "user-agent" {
                continue;
            }

            if let Ok(val) = tonic::metadata::MetadataValue::from_str(value)
                && let Ok(key) = tonic::metadata::MetadataKey::from_str(key)
            {
                metadata.insert(key, val);
            }
        }
    }
}

#[cfg(test)]
pub fn build_metadata(config: &GrpcClientConfig) -> tonic::metadata::MetadataMap {
    let mut metadata = tonic::metadata::MetadataMap::new();
    let user_agent = resolve_user_agent(config.user_agent.as_deref(), config.metadata.as_ref());
    insert_user_agent(&mut metadata, &user_agent);
    insert_custom_metadata(&mut metadata, config.metadata.as_ref());
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::tls::{CompressionMode, WireProtocol};

    fn base_config(metadata: Option<HashMap<String, String>>) -> GrpcClientConfig {
        GrpcClientConfig {
            address: "localhost:50051".to_string(),
            timeout_seconds: 30,
            tls_config: None,
            proto_config: None,
            metadata,
            target_service: None,
            compression: CompressionMode::None,
            connection_id: 0,
            protocol: WireProtocol::Grpc,
                user_agent: None,
        }
    }

    #[test]
    fn build_metadata_uses_default_user_agent() {
        let config = base_config(None);
        let metadata = build_metadata(&config);
        let ua = metadata
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        assert_eq!(ua, user_agent_value());
    }

    #[test]
    fn build_metadata_uses_custom_user_agent_case_insensitive() {
        let mut map = HashMap::new();
        map.insert("User-Agent".to_string(), "custom-client/1.0".to_string());
        map.insert("x-request-id".to_string(), "abc-123".to_string());

        let config = base_config(Some(map));
        let metadata = build_metadata(&config);
        let ua = metadata
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        assert_eq!(ua, "custom-client/1.0");
        assert_eq!(
            metadata
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "abc-123"
        );
    }

    #[tokio::test]
    #[cfg(not(miri))]
    async fn profile_channel_cache_hit() {
        let config = GrpcClientConfig {
            address: "http://localhost:14777".to_string(),
            timeout_seconds: 1,
            tls_config: None,
            proto_config: None,
            metadata: None,
            target_service: None,
            compression: Default::default(),
            connection_id: 0,
            protocol: WireProtocol::Grpc,
                user_agent: None,
        };

        let start = std::time::Instant::now();
        let r1 = create_channel(&config).await;
        let d1 = start.elapsed();
        eprintln!(
            "channel[miss]: {:?} ({})",
            d1,
            if r1.is_ok() { "ok" } else { "err" }
        );

        let start = std::time::Instant::now();
        let r2 = create_channel(&config).await;
        let d2 = start.elapsed();
        eprintln!(
            "channel[hit]:  {:?} ({})",
            d2,
            if r2.is_ok() { "ok" } else { "err" }
        );

        assert!(
            d2 < d1 || d2.as_micros() < 1000,
            "cache hit should be faster than miss"
        );
    }
}
