use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct GrpcError {
    pub code: u32,
    pub message: String,
    pub details: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

impl GrpcError {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
            metadata: HashMap::new(),
        }
    }
    pub fn with_details(code: u32, message: impl Into<String>, details: Vec<u8>) -> Self {
        Self {
            code,
            message: message.into(),
            details,
            metadata: HashMap::new(),
        }
    }
    pub fn with_metadata(
        code: u32,
        message: impl Into<String>,
        details: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details,
            metadata,
        }
    }
    pub fn code(&self) -> u32 {
        self.code
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn details(&self) -> &[u8] {
        &self.details
    }
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }
}

impl fmt::Display for GrpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gRPC error code={} message={}", self.code, self.message)
    }
}
impl std::error::Error for GrpcError {}
