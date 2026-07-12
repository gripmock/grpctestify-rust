use super::proxy::ProxyEnv;
use crate::config::{GrpcClientConfig, TlsConfig};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock, RwLock};
use std::time::Duration;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChannelCacheKey {
    address: String,
    timeout_seconds: u64,
    tls_config: Option<TlsConfig>,
}

static CHANNEL_CACHE: LazyLock<RwLock<HashMap<ChannelCacheKey, Channel>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static PROXY_WARNED: OnceLock<()> = OnceLock::new();

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

    let mut cache_key = ChannelCacheKey {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
    };
    if config.connection_id > 0 {
        cache_key.address = format!("{}/conn-{}", cache_key.address, config.connection_id);
    }

    if let Ok(cache) = CHANNEL_CACHE.read()
        && let Some(channel) = cache.get(&cache_key)
    {
        return Ok(channel.clone());
    }

    PROXY_WARNED.get_or_init(|| ProxyEnv::from_env().warn_if_set());
    let channel = if let Some(tls_config) = &config.tls_config {
        create_tls_channel(config, tls_config).await?
    } else {
        create_plaintext_channel(config).await?
    };

    CHANNEL_CACHE
        .write()
        .map(|mut cache| cache.insert(cache_key, channel.clone()))
        .ok();
    Ok(channel)
}

async fn create_tls_channel(config: &GrpcClientConfig, tls_config: &TlsConfig) -> Result<Channel> {
    let mut tls = ClientTlsConfig::new();
    if let Some(domain) = &tls_config.server_name {
        tls = tls.domain_name(domain);
    }
    if let Some(ca_path) = &tls_config.ca_cert_path {
        tls = tls.ca_certificate(Certificate::from_pem(
            std::fs::read_to_string(ca_path).context("Failed to read CA certificate")?,
        ));
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
            "SECURITY WARNING: TLS certificate verification is disabled (insecure_skip_verify=true)."
        );
    }
    let addr = if !config.address.contains("://") {
        format!("https://{}", config.address)
    } else {
        config.address.clone()
    };
    let endpoint = Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(config.timeout_seconds));
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
    let endpoint = Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(config.timeout_seconds));
    Ok(endpoint.connect_lazy())
}
