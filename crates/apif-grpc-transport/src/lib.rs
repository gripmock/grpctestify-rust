pub mod client;
pub mod config;
pub mod error;
pub mod types;

pub use client::{GrpcClient, GrpcClientFactory};
pub use config::{CompressionMode, GrpcClientConfig, ProtoConfig, TlsConfig, WireProtocol};
pub use error::GrpcError;
pub use types::{EndpointMeta, GrpcResponse, MethodInfo, RpcMode, StreamItem};

#[cfg(feature = "tonic-transport")]
pub mod tonic;
