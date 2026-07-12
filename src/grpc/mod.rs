// gRPC client module — re-exports from apif-grpc-transport

pub mod adapter;
pub mod client;
pub mod grpcurl_invocation;
pub mod proxy;
pub mod web;

pub use apif_grpc_transport::config::{
    CompressionMode, GrpcClientConfig, ProtoConfig, TlsConfig, WireProtocol,
};
pub use apif_grpc_transport::error::GrpcError;
pub use apif_grpc_transport::tonic::client::TonicGrpcClient;
pub use apif_grpc_transport::types::{EndpointMeta, GrpcResponse, MethodInfo, RpcMode, StreamItem};
pub use client::GrpcClient;
