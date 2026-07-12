use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub enum StreamItem {
    Message(Value),
    Trailers(HashMap<String, String>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GrpcResponse {
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub messages: Vec<Value>,
    pub error: Option<String>,
}

impl GrpcResponse {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    pub full_name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcMode {
    Unary,
    ServerStream,
    ClientStream,
    Bidi,
}

#[derive(Debug, Clone)]
pub struct EndpointMeta {
    pub rpc_mode: RpcMode,
    pub input_type: String,
    pub output_type: String,
}
