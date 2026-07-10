// gRPC client module

pub mod adapter;
pub mod channel;
pub mod client;
pub mod grpcurl_invocation;
pub mod proxy;
pub mod tls;

pub use channel::create_channel;
pub use client::{GrpcClient, GrpcClientConfig};
pub use tls::{CompressionMode, ProtoConfig, TlsConfig};

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// gRPC response containing metadata and messages
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct GrpcResponse {
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub messages: Vec<Value>,
    /// Captured error message if the gRPC call ended with an error
    pub error: Option<String>,
}

impl GrpcResponse {
    pub fn new() -> Self {
        Self::default()
    }
}
