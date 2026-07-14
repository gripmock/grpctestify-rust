use anyhow::Result;
use async_trait::async_trait;
use futures::stream::Stream;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;

use crate::config::GrpcClientConfig;
use crate::error::GrpcError;
use crate::types::{EndpointMeta, MethodInfo, StreamItem};

#[async_trait]
pub trait GrpcClient: Send {
    async fn call_stream(
        &mut self,
        service_name: &str,
        method_name: &str,
        requests: Pin<Box<dyn Stream<Item = Value> + Send>>,
    ) -> Result<
        (
            HashMap<String, String>,
            Pin<Box<dyn Stream<Item = Result<StreamItem, GrpcError>> + Send>>,
        ),
        GrpcError,
    >;
    fn list_services(&self) -> Vec<String>;
    fn list_methods(&self, service_name: &str) -> Vec<MethodInfo>;
    fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta, GrpcError>;
    fn generate_schema(&self, endpoint: &str) -> Result<Value, GrpcError>;
}

#[async_trait]
pub trait GrpcClientFactory: Send + Sync {
    async fn create_client(&self, config: GrpcClientConfig) -> Result<Box<dyn GrpcClient>>;
}
