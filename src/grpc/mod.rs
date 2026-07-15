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
pub use apif_grpc_transport::transport::{TransportResult, default_address_for};
pub use apif_grpc_transport::types::{EndpointMeta, GrpcResponse, MethodInfo, RpcMode, StreamItem};
pub use client::GrpcClient;

use anyhow::Result;
use apif_grpc_transport::client::GrpcClient as GrpcClientTrait;
use serde_json::Value;
use std::collections::HashMap;

pub enum TransportRef {
    Tonic(Box<dyn GrpcClientTrait>),
    Http,
}

impl TransportRef {
    pub async fn new(config: &GrpcClientConfig) -> Result<Self> {
        match config.protocol {
            WireProtocol::Grpc => {
                let client = TonicGrpcClient::new(config.clone()).await?;
                Ok(TransportRef::Tonic(Box::new(client)))
            }
            WireProtocol::GrpcWeb | WireProtocol::ConnectRpc => Ok(TransportRef::Http),
        }
    }

    pub async fn execute(
        &mut self,
        config: &GrpcClientConfig,
        service: &str,
        method: &str,
        body: Value,
        rpc_mode: Option<RpcMode>,
    ) -> TransportResult {
        match self {
            TransportRef::Tonic(client) => execute_tonic(client, service, method, body).await,
            TransportRef::Http => {
                match web::execute_web_with_mode(config, service, method, body, rpc_mode).await {
                    Ok(resp) => resp.into(),
                    Err(e) => TransportResult {
                        messages: vec![],
                        headers: HashMap::new(),
                        trailers: HashMap::new(),
                        error: Some(e.to_string()),
                    },
                }
            }
        }
    }
}

async fn execute_tonic(
    client: &mut Box<dyn GrpcClientTrait>,
    service: &str,
    method: &str,
    body: Value,
) -> TransportResult {
    use crate::grpc::StreamItem;
    let stream = Box::pin(futures::stream::iter(vec![body]));
    let (headers, mut response_stream) = match client.call_stream(service, method, stream).await {
        Ok(r) => r,
        Err(e) => {
            return TransportResult {
                messages: vec![],
                headers: HashMap::new(),
                trailers: HashMap::new(),
                error: Some(format!(
                    "gRPC error: code={} message={}",
                    e.code(),
                    e.message()
                )),
            };
        }
    };
    let mut messages = Vec::new();
    let mut trailers = HashMap::new();
    let mut error = None;
    use futures::StreamExt;
    while let Some(item) = response_stream.next().await {
        match item {
            Ok(StreamItem::Message(msg)) => messages.push(msg),
            Ok(StreamItem::Trailers(t)) => {
                trailers.extend(t.clone());
                if let Some(status) = t.get("grpc-status")
                    && status != "0"
                {
                    let msg = t.get("grpc-message").cloned().unwrap_or_default();
                    error = Some(format!("gRPC error: code={} message={}", status, msg));
                }
            }
            Err(s) => {
                error = Some(format!(
                    "gRPC error: code={} message={}",
                    s.code(),
                    s.message()
                ))
            }
        }
    }
    TransportResult {
        messages,
        headers,
        trailers,
        error,
    }
}
