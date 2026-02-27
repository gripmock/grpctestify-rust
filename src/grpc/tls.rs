// TLS configuration utilities

use anyhow::{Context, Result};
use std::collections::HashMap;
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TlsConfig {
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
    pub server_name: Option<String>,
    pub insecure_skip_verify: bool,
}

impl TlsConfig {
    /// Create a new TlsConfig with default values
    pub fn new() -> Self {
        Self {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: false,
        }
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

    /// Build a ClientTlsConfig from this TlsConfig
    pub fn build(&self) -> Result<ClientTlsConfig> {
        let mut tls = ClientTlsConfig::new();

        if let Some(domain) = &self.server_name {
            tls = tls.domain_name(domain);
        }

        if let Some(ca_path) = &self.ca_cert_path {
            let ca_pem =
                std::fs::read_to_string(ca_path).context("Failed to read CA certificate")?;
            tls = tls.ca_certificate(Certificate::from_pem(ca_pem));
        }

        if let (Some(cert_path), Some(key_path)) = (&self.client_cert_path, &self.client_key_path) {
            let cert_pem =
                std::fs::read_to_string(cert_path).context("Failed to read client certificate")?;
            let key_pem = std::fs::read_to_string(key_path).context("Failed to read client key")?;
            tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
        }

        // Security warning: TLS verification disabled
        if self.insecure_skip_verify {
            tracing::warn!(
                "⚠️  SECURITY WARNING: TLS certificate verification is disabled (insecure_skip_verify=true). \
                This is insecure and should only be used for testing or development."
            );
            eprintln!("⚠️  WARNING: TLS verification disabled - this is insecure!");
        }

        Ok(tls)
    }

    /// Check if TLS is enabled (has any TLS configuration)
    pub fn is_enabled(&self) -> bool {
        self.ca_cert_path.is_some()
            || self.client_cert_path.is_some()
            || self.client_key_path.is_some()
            || self.server_name.is_some()
            || self.insecure_skip_verify
    }

    /// Check if insecure mode is enabled
    pub fn is_insecure(&self) -> bool {
        self.insecure_skip_verify
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Proto descriptor configuration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProtoConfig {
    pub files: Vec<String>,
    pub import_paths: Vec<String>,
    pub descriptor: Option<String>,
}

impl ProtoConfig {
    /// Create a new ProtoConfig with default values
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            import_paths: Vec::new(),
            descriptor: None,
        }
    }

    /// Add a proto file
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.files.push(file.into());
        self
    }

    /// Add multiple proto files
    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files.extend(files);
        self
    }

    /// Add an import path
    pub fn with_import_path(mut self, path: impl Into<String>) -> Self {
        self.import_paths.push(path.into());
        self
    }

    /// Set the descriptor file path
    pub fn with_descriptor(mut self, path: impl Into<String>) -> Self {
        self.descriptor = Some(path.into());
        self
    }

    /// Check if proto configuration is empty
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.descriptor.is_none()
    }
}

impl Default for ProtoConfig {
    fn default() -> Self {
        Self::new()
    }
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
        match std::env::var("GRPCTESTIFY_COMPRESSION")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "gzip" => Self::Gzip,
            _ => Self::None,
        }
    }

    /// Check if compression is enabled
    pub fn is_enabled(&self) -> bool {
        matches!(self, CompressionMode::Gzip)
    }

    /// Get the compression encoding for tonic
    pub fn to_tonic_encoding(&self) -> Option<tonic::codec::CompressionEncoding> {
        match self {
            CompressionMode::Gzip => Some(tonic::codec::CompressionEncoding::Gzip),
            CompressionMode::None => None,
        }
    }
}
