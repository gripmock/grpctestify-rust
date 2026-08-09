use super::proxy::ProxyEnv;
use crate::config::{GrpcClientConfig, TlsConfig};
use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::sync::RwLock;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChannelCacheKey {
    address: String,
    timeout_seconds: u64,
    tls_config: Option<TlsConfig>,
    connection_id: u64,
}

/// Bounded channel cache with insertion-order eviction. Clearing on overflow
/// would wipe channels other workers are still using.
struct BoundedChannelCache {
    map: HashMap<ChannelCacheKey, Channel>,
    order: VecDeque<ChannelCacheKey>,
    capacity: usize,
}

impl BoundedChannelCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn get(&self, key: &ChannelCacheKey) -> Option<&Channel> {
        self.map.get(key)
    }

    fn insert(&mut self, key: ChannelCacheKey, channel: Channel) {
        if self.map.insert(key.clone(), channel).is_none() {
            self.order.push_back(key);
        }
        while self.map.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
    }
}

static CHANNEL_CACHE: LazyLock<RwLock<BoundedChannelCache>> =
    LazyLock::new(|| RwLock::new(BoundedChannelCache::new(CHANNEL_CACHE_MAX_ENTRIES)));
static PROXY_WARNED: OnceLock<()> = OnceLock::new();

/// Upper bound on cached channels. Sized so a large `--connections` pool fits
/// without evicting live members; channels connect lazily, so an idle entry is
/// little more than its configuration.
const CHANNEL_CACHE_MAX_ENTRIES: usize = 512;

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

    let cache_key = ChannelCacheKey {
        address: config.address.clone(),
        timeout_seconds: config.timeout_seconds,
        tls_config: config.tls_config.clone(),
        connection_id: config.connection_id,
    };

    {
        let cache = CHANNEL_CACHE.read().await;
        if let Some(channel) = cache.get(&cache_key) {
            tracing::debug!("channel: reusing cached connection to {}", config.address);
            return Ok(channel.clone());
        }
    }

    PROXY_WARNED.get_or_init(|| ProxyEnv::from_env().warn_if_set());
    let channel = if let Some(tls_config) = &config.tls_config {
        tracing::debug!(
            "channel: connecting to {} with TLS (server_name={:?}, client_cert={}, insecure_skip_verify={})",
            config.address,
            tls_config.server_name,
            tls_config.client_cert_path.is_some(),
            tls_config.insecure_skip_verify
        );
        create_tls_channel(config, tls_config).await?
    } else {
        tracing::debug!(
            "channel: connecting to {} without TLS (plaintext)",
            config.address
        );
        create_plaintext_channel(config).await?
    };

    let mut cache = CHANNEL_CACHE.write().await;
    cache.insert(cache_key, channel.clone());
    Ok(channel)
}

/// Endpoint settings shared by both transports. HTTP/2 window sizes are left at
/// their defaults: tuning them measured as a null result on loopback.
fn tune(
    endpoint: tonic::transport::Endpoint,
    config: &GrpcClientConfig,
) -> tonic::transport::Endpoint {
    endpoint
        .timeout(Duration::from_secs(config.timeout_seconds))
        .connect_timeout(Duration::from_secs(5))
}

/// `host:port` with the scheme the transport needs, left alone if it has one.
fn endpoint_uri(address: &str, scheme: &str) -> String {
    if address.contains("://") {
        address.to_string()
    } else {
        format!("{scheme}://{address}")
    }
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
    let endpoint = Channel::from_shared(endpoint_uri(&config.address, "https"))
        .context("Invalid address format")?;
    let endpoint = tune(endpoint, config);
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
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
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
    let endpoint = Channel::from_shared(endpoint_uri(&config.address, "http"))
        .context("Invalid address format")?;
    Ok(tune(endpoint, config).connect_lazy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_key_of(connection_id: u64) -> ChannelCacheKey {
        ChannelCacheKey {
            address: "localhost:50051".to_string(),
            timeout_seconds: 30,
            tls_config: None,
            connection_id,
        }
    }

    // `connection_id` used to be folded into the address only when non-zero, so
    // slot 0 shared a channel with every other caller reaching the same address
    // — including `call`, `health` and `reflect`, which all pass 0.
    fn dummy_channel() -> Channel {
        Channel::from_static("http://127.0.0.1:1").connect_lazy()
    }

    // Overflow used to `clear()` the whole map: with a connection pool larger
    // than the cap, every insert wiped the channels the other workers were
    // still using, so the pool never converged.
    #[tokio::test]
    async fn overflowing_the_cache_evicts_only_the_oldest() {
        let mut cache = BoundedChannelCache::new(3);
        for id in 0..3 {
            cache.insert(cache_key_of(id), dummy_channel());
        }
        cache.insert(cache_key_of(3), dummy_channel());

        assert!(cache.get(&cache_key_of(0)).is_none(), "oldest must go");
        for id in 1..=3 {
            assert!(cache.get(&cache_key_of(id)).is_some(), "slot {id} survives");
        }
        assert_eq!(cache.map.len(), 3);
    }

    // Re-inserting a live key must not queue it twice, or the eviction order
    // drifts and the map shrinks below its capacity.
    #[tokio::test]
    async fn reinserting_a_key_does_not_double_count_it() {
        let mut cache = BoundedChannelCache::new(2);
        cache.insert(cache_key_of(0), dummy_channel());
        cache.insert(cache_key_of(0), dummy_channel());
        cache.insert(cache_key_of(1), dummy_channel());

        assert_eq!(cache.map.len(), 2);
        assert_eq!(cache.order.len(), 2);
        assert!(cache.get(&cache_key_of(0)).is_some());
        assert!(cache.get(&cache_key_of(1)).is_some());
    }

    #[test]
    fn channel_cache_key_separates_every_connection_slot() {
        assert_ne!(cache_key_of(0), cache_key_of(1));
        assert_ne!(cache_key_of(1), cache_key_of(2));
        assert_eq!(cache_key_of(3), cache_key_of(3));
    }

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
