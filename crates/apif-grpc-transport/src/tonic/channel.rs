use super::proxy::ProxyEnv;
use crate::config::{GrpcClientConfig, TlsConfig};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::sync::RwLock;
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

/// Upper bound on cached channels. When the cap is reached the cache is
/// cleared; channels are cheap to recreate (lazy connect) and this prevents
/// unbounded growth over long runs against many distinct addresses.
const CHANNEL_CACHE_MAX_ENTRIES: usize = 64;

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

    {
        let cache = CHANNEL_CACHE.read().await;
        if let Some(channel) = cache.get(&cache_key) {
            return Ok(channel.clone());
        }
    }

    PROXY_WARNED.get_or_init(|| ProxyEnv::from_env().warn_if_set());
    let channel = if let Some(tls_config) = &config.tls_config {
        create_tls_channel(config, tls_config).await?
    } else {
        create_plaintext_channel(config).await?
    };

    let mut cache = CHANNEL_CACHE.write().await;
    if cache.len() >= CHANNEL_CACHE_MAX_ENTRIES {
        cache.clear();
    }
    cache.insert(cache_key, channel.clone());
    Ok(channel)
}

async fn create_tls_channel(config: &GrpcClientConfig, tls_config: &TlsConfig) -> Result<Channel> {
    let mut tls = ClientTlsConfig::new();
    if let Some(domain) = &tls_config.server_name {
        tls = tls.domain_name(domain);
    }
    if let (Some(cert_path), Some(key_path)) =
        (&tls_config.client_cert_path, &tls_config.client_key_path)
    {
        let cert_pem =
            std::fs::read_to_string(cert_path).context("Failed to read client certificate")?;
        let key_pem = std::fs::read_to_string(key_path).context("Failed to read client key")?;
        tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
    }
    let addr = if !config.address.contains("://") {
        format!("https://{}", config.address)
    } else {
        config.address.clone()
    };
    let endpoint = Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(config.timeout_seconds))
        .connect_timeout(Duration::from_secs(5));
    if tls_config.insecure_skip_verify {
        tracing::warn!(
            "SECURITY WARNING: TLS certificate verification is disabled (insecure_skip_verify=true)."
        );
        // A custom verifier replaces the default one entirely, so CA
        // certificates/trust anchors must not be set alongside it (tonic
        // rejects that combination). Client identity and SNI still apply.
        return Ok(endpoint
            .tls_config_with_verifier(tls, insecure::danger_accept_any_server_cert())
            .context("Failed to configure TLS (insecure)")?
            .connect_lazy());
    }
    if let Some(ca_path) = &tls_config.ca_cert_path {
        tls = tls.ca_certificate(Certificate::from_pem(
            std::fs::read_to_string(ca_path).context("Failed to read CA certificate")?,
        ));
    }
    Ok(endpoint
        .tls_config(tls)
        .context("Failed to configure TLS")?
        .connect_lazy())
}

/// Support for `insecure_skip_verify` (explicit user opt-in, equivalent to
/// `grpcurl -insecure`): a rustls server-certificate verifier that accepts
/// any certificate. Signature verification is also skipped, matching the
/// semantics of "do not verify the peer".
mod insecure {
    use rustls::DigitallySignedStruct;
    use rustls::SignatureScheme;
    use rustls::client::danger::{
        HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
    };
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use std::sync::Arc;

    #[derive(Debug)]
    struct DangerAcceptAnyServerCert;

    pub(super) fn danger_accept_any_server_cert() -> Arc<dyn ServerCertVerifier> {
        Arc::new(DangerAcceptAnyServerCert)
    }

    impl ServerCertVerifier for DangerAcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
}

async fn create_plaintext_channel(config: &GrpcClientConfig) -> Result<Channel> {
    let addr = if !config.address.contains("://") {
        format!("http://{}", config.address)
    } else {
        config.address.clone()
    };
    let endpoint = Channel::from_shared(addr)
        .context("Invalid address format")?
        .timeout(Duration::from_secs(config.timeout_seconds))
        .connect_timeout(Duration::from_secs(5));
    Ok(endpoint.connect_lazy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insecure_skip_verify_builds_channel() {
        // Regression test: insecure_skip_verify must actually configure a
        // skip-verify TLS channel (previously it only logged a warning).
        let config = GrpcClientConfig {
            address: "localhost:50051".to_string(),
            tls_config: Some(TlsConfig {
                insecure_skip_verify: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = create_tls_channel(&config, config.tls_config.as_ref().unwrap()).await;
        assert!(
            result.is_ok(),
            "insecure TLS channel should build: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn insecure_skip_verify_ignores_ca_path() {
        // With verification disabled, a CA path (even an unreadable one) must
        // not be loaded — the custom verifier replaces the default verifier.
        let config = GrpcClientConfig {
            address: "localhost:50051".to_string(),
            tls_config: Some(TlsConfig {
                insecure_skip_verify: true,
                ca_cert_path: Some("/nonexistent/ca.pem".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = create_tls_channel(&config, config.tls_config.as_ref().unwrap()).await;
        assert!(result.is_ok(), "CA path must be ignored in insecure mode");
    }

    #[tokio::test]
    async fn secure_tls_channel_builds() {
        let config = GrpcClientConfig {
            address: "localhost:50051".to_string(),
            tls_config: Some(TlsConfig::default()),
            ..Default::default()
        };
        let result = create_tls_channel(&config, config.tls_config.as_ref().unwrap()).await;
        assert!(result.is_ok(), "default TLS channel should build");
    }
}
