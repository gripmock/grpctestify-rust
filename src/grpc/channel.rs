use anyhow::{Context, Result};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::RwLock;
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

use crate::grpc::tls::{ChannelCacheKey, GrpcClientConfig, TlsConfig};

lazy_static::lazy_static! {
    static ref CHANNEL_CACHE: RwLock<HashMap<ChannelCacheKey, Channel>> = RwLock::new(HashMap::new());
}

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

    let user_agent = resolve_user_agent(config.metadata.as_ref());

    let cache_key = ChannelCacheKey {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        user_agent: user_agent.clone(),
    };

    {
        let cache = CHANNEL_CACHE.read().unwrap();
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
        create_tls_channel(config, tls_config, user_agent).await?
    } else {
        create_plaintext_channel(config, user_agent).await?
    };

    {
        let mut cache = CHANNEL_CACHE.write().unwrap();
        cache.insert(cache_key, channel.clone());
    }

    Ok(channel)
}

async fn create_tls_channel(
    config: &GrpcClientConfig,
    tls_config: &TlsConfig,
    user_agent: String,
) -> Result<Channel> {
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
            "⚠️  SECURITY WARNING: TLS certificate verification is disabled (insecure_skip_verify=true). \
            This is insecure and should only be used for testing or development."
        );
        eprintln!("⚠️  WARNING: TLS verification disabled - this is insecure!");
    }

    let addr = if !config.address.contains("://") {
        format!("https://{}", config.address)
    } else {
        config.address.clone()
    };

    let endpoint = Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(config.timeout_seconds))
        .user_agent(user_agent)
        .context("Invalid user-agent value")?;

    Ok(endpoint
        .tls_config(tls)
        .context("Failed to configure TLS")?
        .connect_lazy())
}

async fn create_plaintext_channel(
    config: &GrpcClientConfig,
    user_agent: String,
) -> Result<Channel> {
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

    Ok(endpoint.connect_lazy())
}

fn user_agent_value() -> String {
    format!("grpctestify/{}", env!("CARGO_PKG_VERSION"))
}

fn resolve_user_agent(custom_metadata: Option<&HashMap<String, String>>) -> String {
    if let Some(metadata) = custom_metadata
        && let Some(ua) = metadata.get("user-agent")
    {
        return ua.clone();
    }
    user_agent_value()
}

/// Insert user agent into metadata map
fn insert_user_agent(metadata: &mut tonic::metadata::MetadataMap, user_agent: &str) {
    if let Ok(val) = tonic::metadata::MetadataValue::from_str(user_agent) {
        metadata.insert("user-agent", val);
    }
}

/// Insert custom metadata into metadata map
fn insert_custom_metadata(
    metadata: &mut tonic::metadata::MetadataMap,
    custom: Option<&HashMap<String, String>>,
) {
    if let Some(map) = custom {
        for (key, value) in map {
            // Skip user-agent as it's handled separately
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

/// Build metadata map from config
pub fn build_metadata(config: &GrpcClientConfig) -> tonic::metadata::MetadataMap {
    let mut metadata = tonic::metadata::MetadataMap::new();
    let user_agent = resolve_user_agent(config.metadata.as_ref());
    insert_user_agent(&mut metadata, &user_agent);
    insert_custom_metadata(&mut metadata, config.metadata.as_ref());
    metadata
}
