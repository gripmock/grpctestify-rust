use serde_json::Value;
use std::collections::HashMap;

use crate::config::WireProtocol;
use crate::error::GrpcError;

#[derive(Debug, Default)]
pub struct TransportResult {
    pub messages: Vec<Value>,
    pub message_offsets_ms: Vec<u64>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub error: Option<GrpcError>,
}

pub fn default_address_for(protocol: WireProtocol) -> &'static str {
    match protocol {
        WireProtocol::Grpc => "localhost:4770",
        WireProtocol::GrpcWeb => "localhost:4769",
        WireProtocol::ConnectRpc => "localhost:4769",
    }
}
