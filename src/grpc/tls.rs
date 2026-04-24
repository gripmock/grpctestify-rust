// TLS configuration utilities

use std::collections::HashMap;

/// Key for channel cache
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelCacheKey {
    pub address: String,
    pub timeout_seconds: u64,
    pub tls_config: Option<TlsConfig>,
    pub user_agent: String,
}

/// Configuration for gRPC client
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

/// TLS configuration for gRPC connections
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TlsConfig {
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
    pub server_name: Option<String>,
    pub insecure_skip_verify: bool,
}

impl TlsConfig {
    /// Check if TLS config is empty
    pub fn is_empty(&self) -> bool {
        self.ca_cert_path.is_none()
            && self.client_cert_path.is_none()
            && self.client_key_path.is_none()
            && self.server_name.is_none()
            && !self.insecure_skip_verify
    }

    /// Set the CA certificate path
    pub fn with_ca_cert(mut self, path: impl Into<String>) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }

    /// Set the client certificate and key paths
    pub fn with_client_cert(
        mut self,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
    ) -> Self {
        self.client_cert_path = Some(cert_path.into());
        self.client_key_path = Some(key_path.into());
        self
    }

    /// Set the server name for TLS verification
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Enable insecure mode (skip certificate verification)
    pub fn with_insecure(mut self) -> Self {
        self.insecure_skip_verify = true;
        self
    }
}

/// Proto descriptor configuration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ProtoConfig {
    pub files: Vec<String>,
    pub import_paths: Vec<String>,
    pub descriptor: Option<String>,
}

/// Compression mode for gRPC messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionMode {
    #[default]
    None,
    Gzip,
}

impl CompressionMode {
    /// Get compression mode from environment variable
    pub fn from_env() -> Self {
        match std::env::var(crate::config::ENV_GRPCTESTIFY_COMPRESSION)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "gzip" => Self::Gzip,
            "" | "none" => Self::None,
            _ => Self::None,
        }
    }
}
