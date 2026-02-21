// gRPC client module

pub mod client;

pub use client::{CompressionMode, GrpcClient, GrpcClientConfig, ProtoConfig, TlsConfig};

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// gRPC response containing metadata and messages
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GrpcResponse {
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub messages: Vec<Value>,
    /// Captured error message if the gRPC call ended with an error
    pub error: Option<String>,
}

impl Default for GrpcResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl GrpcResponse {
    pub fn new() -> Self {
        Self {
            headers: HashMap::new(),
            trailers: HashMap::new(),
            messages: Vec::new(),
            error: None,
        }
    }
}
