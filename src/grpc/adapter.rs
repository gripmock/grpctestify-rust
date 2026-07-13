use anyhow::Result;
use apif_execution::{
    CallClient, CallClientConfig, CallClientFactory, CallError, CallRequest, CallStreamItem,
    EndpointMeta, RpcMode,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::client::{GrpcClient, GrpcClientConfig, StreamItem};
use crate::grpc::{TlsConfig, WireProtocol};

/// Factory that creates a native gRPC client.
pub struct GrpcClientFactory;

#[async_trait]
impl CallClientFactory for GrpcClientFactory {
    async fn create_client(&self, config: &CallClientConfig) -> Result<Box<dyn CallClient>> {
        let grpc_config = GrpcClientConfig {
            address: config.address.clone(),
            timeout_seconds: config.timeout_seconds,
            tls_config: config.tls.as_ref().map(convert_tls),
            proto_config: None,
            metadata: config.metadata.clone(),
            target_service: None,
            compression: crate::config::compression_from_env(),
            connection_id: 0,
            protocol: WireProtocol::Grpc,
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let client = GrpcClient::new(grpc_config).await?;
        Ok(Box::new(GrpcCallClient(client)))
    }
}

fn convert_tls(tls: &apif_execution::TlsConfig) -> TlsConfig {
    TlsConfig {
        ca_cert_path: tls.ca_cert.clone(),
        client_cert_path: tls.client_cert.clone(),
        client_key_path: tls.client_key.clone(),
        server_name: tls.server_name.clone(),
        insecure_skip_verify: tls.insecure,
    }
}

/// Native gRPC client (tonic).
pub struct GrpcCallClient(GrpcClient);

#[async_trait]
impl CallClient for GrpcCallClient {
    async fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta> {
        resolve_via_pool(self.0.descriptor_pool(), endpoint)
    }

    async fn call(
        &mut self,
        endpoint: &str,
        request: CallRequest,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<CallStreamItem, CallError>> + Send>>>
    {
        let parts: Vec<&str> = endpoint.split('/').collect();
        anyhow::ensure!(parts.len() == 2, "invalid endpoint format: {}", endpoint);
        let (full_service, method_name) = (parts[0].to_string(), parts[1].to_string());

        let request_stream: Pin<Box<dyn futures::Stream<Item = Value> + Send>> = match request {
            CallRequest::Unary(val) => {
                let (tx, rx) = mpsc::channel(1);
                tx.send(val).await.ok();
                Box::pin(ReceiverStream::new(rx))
            }
            CallRequest::Streaming(s) => s,
        };

        let (_headers, stream) = self
            .0
            .call_stream(&full_service, &method_name, request_stream)
            .await?;

        Ok(Box::pin(stream.map(|item| match item {
            Ok(StreamItem::Message(msg)) => Ok(CallStreamItem::Message(msg)),
            Ok(StreamItem::Trailers(t)) => Ok(CallStreamItem::Trailers(t)),
            Err(status) => Err(CallError {
                code: status.code() as i32,
                message: status.message().to_string(),
            }),
        })))
    }
}

/// HTTP-based client for gRPC-Web and ConnectRPC.
pub struct HttpCallClient {
    config: GrpcClientConfig,
}

#[async_trait]
impl CallClient for HttpCallClient {
    async fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta> {
        // Load descriptors via a temporary gRPC client
        let grpc_config = GrpcClientConfig {
            proto_config: self.config.proto_config.clone(),
            target_service: None,
            ..self.config.clone()
        };
        let client = GrpcClient::new(grpc_config).await?;
        resolve_via_pool(client.descriptor_pool(), endpoint)
    }

    async fn call(
        &mut self,
        endpoint: &str,
        request: CallRequest,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<CallStreamItem, CallError>> + Send>>>
    {
        let parts: Vec<&str> = endpoint.split('/').collect();
        anyhow::ensure!(parts.len() == 2, "invalid endpoint: {}", endpoint);
        let (full_service, method_name) = (parts[0].to_string(), parts[1].to_string());

        let value = match request {
            CallRequest::Unary(v) => v,
            CallRequest::Streaming(mut s) => s.next().await.unwrap_or(Value::Null),
        };

        let mut stream =
            super::web::call_unary(&self.config, &full_service, &method_name, value).await?;

        // Collect all messages
        let mut messages = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(msg) => messages.push(msg),
                Err(e) => return Err(anyhow::anyhow!(e)),
            }
        }

        Ok(Box::pin(futures::stream::iter(
            messages
                .into_iter()
                .map(|msg| Ok(CallStreamItem::Message(msg))),
        )))
    }
}

fn resolve_via_pool(pool: &prost_reflect::DescriptorPool, endpoint: &str) -> Result<EndpointMeta> {
    let parts: Vec<&str> = endpoint.split('/').collect();
    anyhow::ensure!(parts.len() == 2, "invalid endpoint: {}", endpoint);
    let (full_service, method_name) = (parts[0], parts[1]);

    let svc = pool
        .get_service_by_name(full_service)
        .ok_or_else(|| anyhow::anyhow!("service not found: {}", full_service))?;
    let m = svc
        .methods()
        .find(|m| m.name() == method_name)
        .ok_or_else(|| anyhow::anyhow!("method not found: {}", method_name))?;

    Ok(EndpointMeta {
        rpc_mode: match (m.is_client_streaming(), m.is_server_streaming()) {
            (false, false) => RpcMode::Unary,
            (false, true) => RpcMode::ServerStream,
            (true, false) => RpcMode::ClientStream,
            (true, true) => RpcMode::Bidi,
        },
        input_type: Some(m.input().full_name().to_string()),
        output_type: Some(m.output().full_name().to_string()),
    })
}
