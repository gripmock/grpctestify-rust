pub mod adapter;
pub mod client;
pub mod grpcurl_invocation;
pub mod proxy;
pub mod template;
pub mod web;
pub(crate) mod web_reflection;

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
        tracing::debug!("dialing {} via {:?}", config.address, config.protocol);
        match config.protocol {
            WireProtocol::Grpc => {
                let client = TonicGrpcClient::new(config.clone()).await?;
                Ok(TransportRef::Tonic(Box::new(client)))
            }
            WireProtocol::GrpcWeb | WireProtocol::ConnectRpc => Ok(TransportRef::Http),
        }
    }

    pub fn method_info(&self, service: &str, method: &str) -> Option<MethodInfo> {
        match self {
            TransportRef::Tonic(client) => client
                .list_methods(service)
                .into_iter()
                .find(|m| m.name == method),
            TransportRef::Http => None,
        }
    }

    pub async fn execute_streaming(
        &mut self,
        config: &GrpcClientConfig,
        service: &str,
        method: &str,
        bodies: Vec<Value>,
        rpc_mode: Option<RpcMode>,
    ) -> TransportResult {
        match self {
            TransportRef::Tonic(client) => {
                execute_tonic_stream(client, service, method, bodies).await
            }
            TransportRef::Http => {
                let body = bodies.into_iter().next().unwrap_or(Value::Null);
                self.execute(config, service, method, body, rpc_mode).await
            }
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
        let result = match self {
            TransportRef::Tonic(client) => execute_tonic(client, service, method, body).await,
            TransportRef::Http => {
                match web::execute_web_with_mode(config, service, method, body, rpc_mode).await {
                    Ok(resp) => resp.into(),
                    Err(e) => TransportResult {
                        messages: vec![],
                        message_offsets_ms: vec![],
                        headers: HashMap::new(),
                        trailers: HashMap::new(),
                        error: Some(GrpcError::new(2, e.to_string())),
                    },
                }
            }
        };
        tracing::debug!(
            "{}/{} -> {} message(s), error={:?}",
            service,
            method,
            result.messages.len(),
            result.error
        );
        result
    }
}

async fn execute_tonic(
    client: &mut Box<dyn GrpcClientTrait>,
    service: &str,
    method: &str,
    body: Value,
) -> TransportResult {
    execute_tonic_stream(client, service, method, vec![body]).await
}

async fn execute_tonic_stream(
    client: &mut Box<dyn GrpcClientTrait>,
    service: &str,
    method: &str,
    bodies: Vec<Value>,
) -> TransportResult {
    use crate::grpc::StreamItem;
    let stream = Box::pin(futures::stream::iter(bodies));
    let (headers, mut response_stream) = match client.call_stream(service, method, stream).await {
        Ok(r) => r,
        Err(e) => {
            return TransportResult {
                messages: vec![],
                message_offsets_ms: vec![],
                headers: HashMap::new(),
                trailers: HashMap::new(),
                error: Some(e),
            };
        }
    };
    let mut messages = Vec::new();
    let mut message_offsets_ms = Vec::new();
    let mut trailers = HashMap::new();
    let mut error: Option<GrpcError> = None;
    use futures::StreamExt;
    let started = std::time::Instant::now();
    while let Some(item) = response_stream.next().await {
        match item {
            Ok(StreamItem::Message(msg)) => {
                messages.push(msg);
                message_offsets_ms.push(started.elapsed().as_millis() as u64);
            }
            Ok(StreamItem::Trailers(t)) => {
                trailers.extend(t.clone());
                if let Some(status) = t.get("grpc-status")
                    && status != "0"
                {
                    let msg = t.get("grpc-message").cloned().unwrap_or_default();
                    error = Some(GrpcError::new(status.parse::<u32>().unwrap_or(2), msg));
                }
            }
            Err(s) => error = Some(s),
        }
    }
    TransportResult {
        messages,
        message_offsets_ms,
        headers,
        trailers,
        error,
    }
}
