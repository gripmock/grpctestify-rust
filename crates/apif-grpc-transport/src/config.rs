use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireProtocol {
    #[default]
    Grpc,
    GrpcWeb,
    ConnectRpc,
}

impl std::str::FromStr for WireProtocol {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "grpc-web" => Self::GrpcWeb,
            "connectrpc" => Self::ConnectRpc,
            _ => Self::Grpc,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TlsConfig {
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
    pub server_name: Option<String>,
    pub insecure_skip_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ProtoConfig {
    pub files: Vec<String>,
    pub import_paths: Vec<String>,
    pub descriptor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionMode {
    #[default]
    None,
    Gzip,
}

#[derive(Debug, Clone)]
pub struct GrpcClientConfig {
    pub address: String,
    pub timeout_seconds: u64,
    pub tls_config: Option<TlsConfig>,
    pub proto_config: Option<ProtoConfig>,
    pub metadata: Option<HashMap<String, String>>,
    pub target_service: Option<String>,
    pub compression: CompressionMode,
    pub connection_id: u64,
    pub protocol: WireProtocol,
    pub version: String,
}

impl Default for GrpcClientConfig {
    fn default() -> Self {
        Self {
            address: String::new(),
            timeout_seconds: 0,
            tls_config: None,
            proto_config: None,
            metadata: None,
            target_service: None,
            compression: CompressionMode::default(),
            connection_id: 0,
            protocol: WireProtocol::default(),
            version: "unknown".to_string(),
        }
    }
}
