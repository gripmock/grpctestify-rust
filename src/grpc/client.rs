use anyhow::Result;
use apif_grpc_transport::client::GrpcClient as _;
use futures::stream::{Stream, StreamExt};
use prost::Message;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;

pub struct GrpcClient {
    inner: ClientInner,
}

enum ClientInner {
    Tonic(apif_grpc_transport::tonic::client::TonicGrpcClient),
    Http {
        config: apif_grpc_transport::config::GrpcClientConfig,
        pool: Option<prost_reflect::DescriptorPool>,
    },
}

impl GrpcClient {
    pub async fn new(config: apif_grpc_transport::config::GrpcClientConfig) -> Result<Self> {
        match config.protocol {
            apif_grpc_transport::config::WireProtocol::Grpc => Ok(Self {
                inner: ClientInner::Tonic(
                    apif_grpc_transport::tonic::client::TonicGrpcClient::new(config).await?,
                ),
            }),
            apif_grpc_transport::config::WireProtocol::GrpcWeb
            | apif_grpc_transport::config::WireProtocol::ConnectRpc => {
                let pool = build_pool_from_config(&config);
                Ok(Self {
                    inner: ClientInner::Http { config, pool },
                })
            }
        }
    }

    pub fn descriptor_pool(&self) -> &prost_reflect::DescriptorPool {
        match &self.inner {
            ClientInner::Tonic(c) => c.descriptor_pool(),
            ClientInner::Http { pool, .. } => pool.as_ref().unwrap_or(&EMPTY_POOL),
        }
    }

    pub fn describe(&self, symbol: Option<&str>) -> Result<String> {
        match &self.inner {
            ClientInner::Tonic(c) => {
                let pool = c.descriptor_pool();
                Self::describe_pool(pool, symbol)
            }
            ClientInner::Http { .. } => {
                anyhow::bail!("describe is not supported for HTTP transport")
            }
        }
    }

    fn describe_pool(pool: &prost_reflect::DescriptorPool, symbol: Option<&str>) -> Result<String> {
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
        rpc_mode: Option<RpcMode>,
    ) -> Result<(
        HashMap<String, String>,
        Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send + 'static>>,
    )> {
        match &mut self.inner {
            ClientInner::Tonic(c) => {
                let (headers, stream) = c
                    .call_stream(service_name, method_name, Box::pin(requests))
                    .await?;
                Ok((headers, Box::pin(stream)))
            }
            ClientInner::Http { config, .. } => {
                use crate::grpc::TransportRef;
                use futures::stream;

                // For streaming modes where all requests arrive before first response
                // (client-streaming, bidi), collect ALL and send as envelope stream.
                // For unary/server-streaming, read first request immediately to avoid deadlock.
                let needs_collect = matches!(
                    rpc_mode,
                    Some(crate::grpc::RpcMode::ClientStream | crate::grpc::RpcMode::Bidi)
                );
                let request_body = if needs_collect {
                    let all: Vec<Value> = requests.collect().await;
                    if all.is_empty() {
                        return Ok((HashMap::new(), Box::pin(stream::iter(vec![]))));
                    }
                    if all.len() == 1 {
                        all.into_iter().next().unwrap()
                    } else {
                        let is_grpc_web = config.protocol == crate::grpc::WireProtocol::GrpcWeb;
                        let (messages, trailers, error, headers) = if is_grpc_web {
                            let body = crate::grpc::web::encode_multi_request_grpc_web(&all);
                            let (_status, response_bytes, headers) =
                                crate::grpc::web::send_http_post(
                                    config,
                                    service_name,
                                    method_name,
                                    "application/grpc-web+json",
                                    &body,
                                )
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e))?;
                            let (mut m, t, mut e) =
                                crate::grpc::web::parse_grpc_web_framed_json_public(
                                    &response_bytes,
                                );
                            // Extract structured error details from data frame if available
                            crate::grpc::web::enrich_grpc_web_error(&mut m, &mut e);
                            (m, t, e, headers)
                        } else {
                            let body = crate::grpc::web::encode_multi_request(
                                &all,
                                config.proto_config.is_some(),
                            );
                            let (_status, response_bytes, headers) =
                                crate::grpc::web::send_http_post(
                                    config,
                                    service_name,
                                    method_name,
                                    "application/connect+json",
                                    &body,
                                )
                                .await
                                .map_err(|e| anyhow::anyhow!("{}", e))?;
                            let (m, t, e) = crate::grpc::web::parse_connect_framed_public(
                                &response_bytes,
                                None,
                                &headers,
                            );
                            (m, t, e, headers)
                        };
                        return Ok((
                            headers,
                            Box::pin(stream::iter(Self::convert_result(
                                messages,
                                trailers,
                                error,
                                HashMap::new(),
                            ))),
                        ));
                    }
                } else {
                    let mut pinned = Box::pin(requests);
                    pinned.next().await.unwrap_or(Value::Null)
                };

                let mut transport = TransportRef::new(config).await?;
                let result = transport
                    .execute(config, service_name, method_name, request_body, rpc_mode)
                    .await;

                let headers = result.headers.clone();
                let messages = result.messages;
                let trailers = result.trailers;
                let error = result.error;

                let items = Self::convert_result(messages, trailers, error, headers.clone());
                Ok((headers, Box::pin(stream::iter(items))))
            }
        }
    }

    pub async fn call(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: Vec<Value>,
    ) -> Result<TestResponse> {
        let stream = futures::stream::iter(requests);
        let (headers, mut stream) = self
            .call_stream(service_name, method_name, stream, None)
            .await?;
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

    /// Convert TransportResult fields into a Vec of StreamItems for unified stream output.
    fn convert_result(
        messages: Vec<Value>,
        trailers: HashMap<String, String>,
        error: Option<String>,
        headers: HashMap<String, String>,
    ) -> Vec<Result<StreamItem, GrpcError>> {
        use crate::grpc::GrpcError;
        let mut items: Vec<Result<StreamItem, GrpcError>> = Vec::new();

        if let Some(err_msg) = error {
            let code = err_msg
                .split("code=")
                .nth(1)
                .and_then(|s| s.split(char::is_whitespace).next())
                .and_then(|s| {
                    s.parse::<u32>().ok().or(Some(match s {
                        "cancelled" => 1,
                        "unknown" => 2,
                        "invalid_argument" => 3,
                        "deadline_exceeded" => 4,
                        "not_found" => 5,
                        "already_exists" => 6,
                        "permission_denied" => 7,
                        "resource_exhausted" => 8,
                        "failed_precondition" => 9,
                        "aborted" => 10,
                        "out_of_range" => 11,
                        "unimplemented" => 12,
                        "internal" => 13,
                        "unavailable" => 14,
                        "data_loss" => 15,
                        "unauthenticated" => 16,
                        _ => 2,
                    }))
                })
                .unwrap_or(2);
            let message = err_msg
                .split("message=")
                .nth(1)
                .and_then(|s| s.split(" details=").next())
                .unwrap_or(&err_msg)
                .to_string();
            let details_bytes = err_msg
                .split("details=[")
                .nth(1)
                .and_then(|s| s.rsplit_once(']'))
                .map(|(json, _)| json.as_bytes().to_vec())
                .unwrap_or_default();
            let mut err_trailers = trailers;
            for (k, v) in &headers {
                err_trailers.entry(k.clone()).or_insert_with(|| v.clone());
            }
            items.push(Err(GrpcError::with_metadata(
                code,
                message,
                details_bytes,
                err_trailers,
            )));
        } else {
            for msg in messages {
                items.push(Ok(StreamItem::Message(msg)));
            }
            if !trailers.is_empty() {
                items.push(Ok(StreamItem::Trailers(trailers)));
            }
        }

        items
    }
}

#[derive(Debug, Clone)]
pub struct TestResponse {
    pub headers: HashMap<String, String>,
    pub messages: Vec<Value>,
    pub trailers: HashMap<String, String>,
}

fn build_pool_from_config(config: &GrpcClientConfig) -> Option<prost_reflect::DescriptorPool> {
    let desc_path = config.proto_config.as_ref()?.descriptor.as_ref()?;
    let desc_bytes = std::fs::read(desc_path).ok()?;
    let fds = prost_types::FileDescriptorSet::decode(&*desc_bytes).ok()?;
    prost_reflect::DescriptorPool::from_file_descriptor_set(fds).ok()
}

static EMPTY_POOL: std::sync::LazyLock<prost_reflect::DescriptorPool> =
    std::sync::LazyLock::new(|| {
        prost_reflect::DescriptorPool::from_file_descriptor_set(
            prost_types::FileDescriptorSet::default(),
        )
        .expect("empty FileDescriptorSet should always be valid")
    });

pub use apif_grpc_transport::config::{
    CompressionMode, GrpcClientConfig, ProtoConfig, TlsConfig, WireProtocol,
};
pub use apif_grpc_transport::error::GrpcError;
pub use apif_grpc_transport::types::{RpcMode, StreamItem};
