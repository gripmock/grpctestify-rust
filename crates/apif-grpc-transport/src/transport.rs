use serde_json::Value;
use std::collections::HashMap;

use crate::config::WireProtocol;
use crate::error::GrpcError;

#[derive(Debug, Default)]
pub struct TransportResult {
    pub messages: Vec<Value>,
    pub headers: HashMap<String, String>,
    pub trailers: HashMap<String, String>,
    /// Structured status carried verbatim across the transport boundary — no
    /// format-then-reparse. Producers build it directly from `tonic::Status`
    /// (grpc) or the parsed Connect/grpc-web error (HTTP).
    pub error: Option<GrpcError>,
}

pub fn default_address_for(protocol: WireProtocol) -> &'static str {
    match protocol {
        WireProtocol::Grpc => "localhost:4770",
        WireProtocol::GrpcWeb => "localhost:4769",
        WireProtocol::ConnectRpc => "localhost:4769",
    }
}
