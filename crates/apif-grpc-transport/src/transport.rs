use serde_json::Value;
use std::collections::HashMap;

use crate::config::WireProtocol;

#[derive(Debug, Default)]
pub struct TransportResult {
    pub messages: Vec<Value>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    pub error: Option<String>,
}

pub fn default_address_for(protocol: WireProtocol) -> &'static str {
    match protocol {
        WireProtocol::Grpc => "localhost:4770",
        WireProtocol::GrpcWeb => "localhost:4769",
        WireProtocol::ConnectRpc => "localhost:4769",
    }
}
