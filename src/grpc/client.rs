use anyhow::Result;
use apif_grpc_transport::client::GrpcClient as _;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;

pub struct GrpcClient {
    inner: apif_grpc_transport::tonic::client::TonicGrpcClient,
}

impl GrpcClient {
    pub async fn new(config: apif_grpc_transport::config::GrpcClientConfig) -> Result<Self> {
        Ok(Self {
            inner: apif_grpc_transport::tonic::client::TonicGrpcClient::new(config).await?,
        })
    }

    pub fn descriptor_pool(&self) -> &prost_reflect::DescriptorPool {
        self.inner.descriptor_pool()
    }

    pub fn describe(&self, symbol: Option<&str>) -> Result<String> {
        let pool = self.inner.descriptor_pool();
        if let Some(sym) = symbol {
            let parts: Vec<&str> = sym.split('/').collect();
            if parts.len() != 2 {
                if let Some(service) = pool.get_service_by_name(sym) {
                    let mut out = format!("Service: {}\n", service.name());
                    for m in service.methods() {
                        out.push_str(&format!(
                            "  rpc {}({}) returns ({});\n",
                            m.name(),
                            m.input().name(),
                            m.output().name()
                        ));
                    }
                    return Ok(out);
                }
                return Ok(format!(
                    "Invalid symbol: {}. Expected 'package.Service/Method' or 'package.Service'",
                    sym
                ));
            }
            let svc = pool
                .get_service_by_name(parts[0])
                .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", parts[0]))?;
            let m = svc
                .methods()
                .find(|m| m.name() == parts[1])
                .ok_or_else(|| anyhow::anyhow!("Method '{}' not found", parts[1]))?;
            Ok(format!(
                "rpc {}({}) returns ({})\n  Input: {}\n  Output: {}",
                m.name(),
                m.input().name(),
                m.output().name(),
                m.input().full_name(),
                m.output().full_name()
            ))
        } else {
            let services: Vec<_> = pool.services().map(|s| s.name().to_string()).collect();
            Ok(format!(
                "Services ({}):\n  - {}",
                services.len(),
                services.join("\n  - ")
            ))
        }
    }

    pub async fn call_stream(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: impl Stream<Item = Value> + Send + 'static,
    ) -> Result<(
        HashMap<String, String>,
        Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send + 'static>>,
    )> {
        let (headers, stream) = self
            .inner
            .call_stream(service_name, method_name, Box::pin(requests))
            .await?;
        Ok((headers, Box::pin(stream)))
    }

    pub async fn call(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: Vec<Value>,
    ) -> Result<TestResponse> {
        let stream = futures::stream::iter(requests);
        let (headers, mut stream) = self
            .inner
            .call_stream(service_name, method_name, Box::pin(stream))
            .await
            .map_err(|e| anyhow::anyhow!("gRPC error: {} {}", e.code, e.message))?;
        let mut messages = Vec::new();
        let mut trailers = HashMap::new();
        while let Some(item) = stream.next().await {
            match item.map_err(|e| anyhow::anyhow!("gRPC error: {} {}", e.code, e.message))? {
                StreamItem::Message(m) => messages.push(m),
                StreamItem::Trailers(t) => trailers.extend(t),
            }
        }
        Ok(TestResponse {
            headers,
            messages,
            trailers,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TestResponse {
    pub headers: HashMap<String, String>,
    pub messages: Vec<Value>,
    pub trailers: HashMap<String, String>,
}

pub use apif_grpc_transport::config::{
    CompressionMode, GrpcClientConfig, ProtoConfig, TlsConfig, WireProtocol,
};
pub use apif_grpc_transport::error::GrpcError;
pub use apif_grpc_transport::types::StreamItem;
