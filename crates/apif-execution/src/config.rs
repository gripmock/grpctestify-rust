use std::collections::HashMap;

/// Protocol-agnostic TLS configuration.
#[derive(Debug, Clone, Default)]
pub struct TlsConfig {
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub server_name: Option<String>,
    pub insecure: bool,
}

/// Configuration for creating a call client.
#[derive(Debug, Clone, Default)]
pub struct CallClientConfig {
    pub address: String,
    pub timeout_seconds: u64,
    pub tls: Option<TlsConfig>,
    pub metadata: Option<HashMap<String, String>>,
    pub compression: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_default() {
        let tls = TlsConfig::default();
        assert!(tls.ca_cert.is_none());
        assert!(tls.client_cert.is_none());
        assert!(!tls.insecure);
    }

    #[test]
    fn test_tls_config_custom() {
        let tls = TlsConfig {
            ca_cert: Some("/path/to/ca.pem".into()),
            insecure: true,
            ..Default::default()
        };
        assert_eq!(tls.ca_cert.as_deref(), Some("/path/to/ca.pem"));
        assert!(tls.insecure);
    }

    #[test]
    fn test_call_client_config_default() {
        let cfg = CallClientConfig::default();
        assert!(cfg.address.is_empty());
        assert_eq!(cfg.timeout_seconds, 0);
        assert!(cfg.tls.is_none());
        assert!(cfg.metadata.is_none());
    }

    #[test]
    fn test_call_client_config_custom() {
        let mut meta = HashMap::new();
        meta.insert("authorization".into(), "token123".into());
        let cfg = CallClientConfig {
            address: "localhost:8080".into(),
            timeout_seconds: 30,
            metadata: Some(meta),
            ..Default::default()
        };
        assert_eq!(cfg.address, "localhost:8080");
        assert_eq!(cfg.timeout_seconds, 30);
        assert_eq!(cfg.metadata.as_ref().unwrap()["authorization"], "token123");
    }
}
