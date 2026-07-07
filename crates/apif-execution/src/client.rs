use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;

/// How the client communicates with the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcMode {
    Unary,
    ServerStream,
    ClientStream,
    Bidi,
}

/// Endpoint metadata resolved by the client.
#[derive(Debug, Clone)]
pub struct EndpointMeta {
    pub rpc_mode: RpcMode,
    pub input_type: Option<String>,
    pub output_type: Option<String>,
}

/// A single item from the response stream.
#[derive(Debug, Clone)]
pub enum CallStreamItem {
    Message(Value),
    Trailers(HashMap<String, String>),
}

/// Protocol-agnostic call error.
#[derive(Debug, Clone)]
pub struct CallError {
    pub code: i32,
    pub message: String,
}

/// How requests are sent to the server.
pub enum CallRequest {
    Unary(Value),
    Streaming(Pin<Box<dyn futures::Stream<Item = Value> + Send>>),
}

/// Protocol-agnostic call client trait.
///
/// Each protocol (gRPC, HTTP) implements this trait.
/// The runner uses this trait instead of directly creating a gRPC client.
#[async_trait]
pub trait CallClient: Send {
    /// Resolve endpoint metadata (RPC mode, input/output types).
    async fn resolve_endpoint(&self, endpoint: &str) -> Result<EndpointMeta>;

    /// Make a call and return a stream of response items.
    async fn call(
        &mut self,
        endpoint: &str,
        request: CallRequest,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<CallStreamItem, CallError>> + Send>>>;
}

/// Factory that creates call clients for specific documents/configs.
#[async_trait]
pub trait CallClientFactory: Send + Sync {
    async fn create_client(&self, config: &crate::config::CallClientConfig) -> Result<Box<dyn CallClient>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_rpc_mode_debug() {
        assert_eq!(format!("{:?}", RpcMode::Unary), "Unary");
        assert_eq!(format!("{:?}", RpcMode::ServerStream), "ServerStream");
        assert_eq!(format!("{:?}", RpcMode::ClientStream), "ClientStream");
        assert_eq!(format!("{:?}", RpcMode::Bidi), "Bidi");
    }

    #[test]
    fn test_call_error_new() {
        let err = CallError { code: 5, message: "not found".into() };
        assert_eq!(err.code, 5);
        assert_eq!(err.message, "not found");
    }

    #[test]
    fn test_call_stream_item_message() {
        let item = CallStreamItem::Message(serde_json::json!({"key": "value"}));
        match item {
            CallStreamItem::Message(v) => assert_eq!(v["key"], "value"),
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn test_call_stream_item_trailers() {
        let mut h = HashMap::new();
        h.insert("x-status".into(), "ok".into());
        let item = CallStreamItem::Trailers(h);
        match item {
            CallStreamItem::Trailers(t) => assert_eq!(t.get("x-status"), Some(&"ok".into())),
            _ => panic!("expected Trailers"),
        }
    }

    #[test]
    fn test_endpoint_meta_new() {
        let meta = EndpointMeta {
            rpc_mode: RpcMode::Unary,
            input_type: Some("test.Request".into()),
            output_type: Some("test.Response".into()),
        };
        assert_eq!(meta.rpc_mode, RpcMode::Unary);
        assert_eq!(meta.input_type.as_deref(), Some("test.Request"));
    }
}

