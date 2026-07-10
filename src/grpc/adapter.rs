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
use super::tls::{CompressionMode, TlsConfig};

/// gRPC factory implementing CallClientFactory.
pub struct GrpcClientFactory;

#[async_trait]
impl CallClientFactory for GrpcClientFactory {
    async fn create_client(&self, config: &CallClientConfig) -> Result<Box<dyn CallClient>> {
        let grpc_config = GrpcClientConfig {
            address: config.address.clone(),
            timeout_seconds: config.timeout_seconds,
            tls_config: config.tls.as_ref().map(convert_tls),
            proto_config: None, // caller sets this separately
            metadata: config.metadata.clone(),
            target_service: None,
            compression: CompressionMode::from_env(),
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

/// gRPC implementation of the CallClient trait.
pub struct GrpcCallClient(GrpcClient);

#[async_trait]
impl CallClient for GrpcCallClient {
    async fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta> {
        let parts: Vec<&str> = endpoint.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!("invalid endpoint format: {}", endpoint);
        }
        let (full_service, method_name) = (parts[0], parts[1]);

        let pool = self.0.descriptor_pool();
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
