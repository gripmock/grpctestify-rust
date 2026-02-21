// AST (Abstract Syntax Tree) for .gctf files
// Represents the parsed structure of a .gctf test file

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete .gctf document
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GctfDocument {
    /// File path (absolute or relative)
    pub file_path: String,

    /// All sections in the document (preserving order)
    pub sections: Vec<Section>,

    /// Document metadata
    pub metadata: DocumentMetadata,
}

/// Document metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Original file content (for error reporting)
    pub source: Option<String>,

    /// File modification time (for caching)
    pub mtime: Option<i64>,

    /// Parsed at timestamp
    pub parsed_at: i64,
}

/// A section in the .gctf file
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Section type
    pub section_type: SectionType,

    /// Content of the section (raw text, typically JSON)
    pub content: SectionContent,

    /// Inline options (for sections that support them)
    pub inline_options: InlineOptions,

    /// Raw text content of the section (preserved for formatting)
    pub raw_content: String,

    /// Line number where section starts
    pub start_line: usize,

    /// Line number where section ends
    pub end_line: usize,
}

/// Section content
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SectionContent {
    /// Single value (ADDRESS, ENDPOINT, etc.)
    Single(String),

    /// JSON object (REQUEST, RESPONSE, ERROR)
    Json(serde_json::Value),

    /// Newline-delimited JSON values within a single section block
    JsonLines(Vec<serde_json::Value>),

    /// Key-value pairs (REQUEST_HEADERS, TLS, OPTIONS, PROTO)
    KeyValues(HashMap<String, String>),

    /// Extract variables from response (EXTRACT)
    Extract(HashMap<String, String>),

    /// Assertion expressions (ASSERTS)
    Assertions(Vec<String>),

    /// Empty section
    Empty,
}

/// Section types in .gctf files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SectionType {
    /// Server address
    Address,

    /// gRPC endpoint (service/method)
    Endpoint,

    /// Request payload (can have multiple)
    Request,

    /// Expected response (can have multiple)
    Response,

    /// Expected error
    Error,

    /// Request-specific headers
    RequestHeaders,

    /// Assertion expressions (can have multiple)
    Asserts,

    /// Protocol buffer configuration
    Proto,

    /// TLS/mTLS configuration
    Tls,

    /// Test execution options
    Options,

    /// Extract variables from response
    Extract,
}

impl SectionType {
    /// Get section name as string
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionType::Address => "ADDRESS",
            SectionType::Endpoint => "ENDPOINT",
            SectionType::Request => "REQUEST",
            SectionType::Response => "RESPONSE",
            SectionType::Error => "ERROR",
            SectionType::RequestHeaders => "REQUEST_HEADERS",
            SectionType::Asserts => "ASSERTS",
            SectionType::Proto => "PROTO",
            SectionType::Tls => "TLS",
            SectionType::Options => "OPTIONS",
            SectionType::Extract => "EXTRACT",
        }
    }

    /// Parse section name string to SectionType
    pub fn from_keyword(s: &str) -> Option<SectionType> {
        match s.trim() {
            "ADDRESS" => Some(SectionType::Address),
            "ENDPOINT" => Some(SectionType::Endpoint),
            "REQUEST" => Some(SectionType::Request),
            "RESPONSE" => Some(SectionType::Response),
            "ERROR" => Some(SectionType::Error),
            "REQUEST_HEADERS" | "HEADERS" => Some(SectionType::RequestHeaders),
            "ASSERTS" => Some(SectionType::Asserts),
            "PROTO" => Some(SectionType::Proto),
            "TLS" => Some(SectionType::Tls),
            "OPTIONS" => Some(SectionType::Options),
            "EXTRACT" => Some(SectionType::Extract),
            _ => None,
        }
    }

    /// Check if section can appear multiple times
    pub fn is_multiple_allowed(&self) -> bool {
        matches!(
            self,
            SectionType::Request
                | SectionType::Response
                | SectionType::Asserts
                | SectionType::Extract
        )
    }

    /// Check if section supports inline options
    pub fn supports_inline_options(&self) -> bool {
        matches!(self, SectionType::Response)
    }
}

/// Inline options for sections
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InlineOptions {
    /// Run ASSERTS on same response (unary RPC)
    pub with_asserts: bool,

    /// Subset comparison (expected is subset of actual)
    pub partial: bool,

    /// Numeric tolerance for floating-point comparisons
    pub tolerance: Option<f64>,

    /// Remove sensitive fields before comparison
    pub redact: Vec<String>,

    /// Sort arrays for order-independent comparison
    pub unordered_arrays: bool,
}

/// GCTF file header with inline options
/// Format: --- SECTION_NAME key=value ... ---
#[derive(Debug, Clone, PartialEq)]
pub struct SectionHeader {
    /// Section type
    pub section_type: SectionType,

    /// Inline options (key=value pairs)
    pub options: HashMap<String, String>,
}

impl GctfDocument {
    /// Create a new empty document
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            sections: Vec::new(),
            metadata: DocumentMetadata {
                source: None,
                mtime: None,
                parsed_at: chrono::Utc::now().timestamp(),
            },
        }
    }

    /// Get all sections of a specific type
    pub fn sections_by_type(&self, section_type: SectionType) -> Vec<&Section> {
        self.sections
            .iter()
            .filter(|s| s.section_type == section_type)
            .collect()
    }

    /// Get first section of a specific type
    pub fn first_section(&self, section_type: SectionType) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.section_type == section_type)
    }

    /// Get address (from ADDRESS section or environment variable)
    pub fn get_address(&self, env_address: Option<&str>) -> Option<String> {
        if let Some(section) = self.first_section(SectionType::Address) {
            if let SectionContent::Single(addr) = &section.content {
                return Some(addr.clone());
            }
        }
        env_address.map(|s| s.to_string())
    }

    /// Get endpoint
    pub fn get_endpoint(&self) -> Option<String> {
        if let Some(section) = self.first_section(SectionType::Endpoint) {
            if let SectionContent::Single(endpoint) = &section.content {
                return Some(endpoint.clone());
            }
        }
        None
    }

    /// Parse endpoint into package, service, method
    pub fn parse_endpoint(&self) -> Option<(String, String, String)> {
        let endpoint = self.get_endpoint()?;
        let parts: Vec<&str> = endpoint.split('/').collect();
        if parts.len() == 2 {
            let full_service = parts[0];
            let service_parts: Vec<&str> = full_service.split('.').collect();
            if service_parts.len() >= 2 {
                let package = service_parts[..service_parts.len() - 1].join(".");
                let service = service_parts[service_parts.len() - 1].to_string();
                let method = parts[1].to_string();
                return Some((package, service, method));
            } else if service_parts.len() == 1 {
                let package = String::new();
                let service = service_parts[0].to_string();
                let method = parts[1].to_string();
                return Some((package, service, method));
            }
        }
        None
    }

    /// Get all request payloads
    pub fn get_requests(&self) -> Vec<serde_json::Value> {
        self.sections_by_type(SectionType::Request)
            .into_iter()
            .filter_map(|s| {
                if let SectionContent::Json(json) = &s.content {
                    Some(json.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all response payloads
    #[allow(dead_code)]
    pub fn get_responses(&self) -> Vec<serde_json::Value> {
        self.sections_by_type(SectionType::Response)
            .into_iter()
            .flat_map(|s| match &s.content {
                SectionContent::Json(json) => vec![json.clone()],
                SectionContent::JsonLines(values) => values.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// Get all assertion sections
    pub fn get_assertions(&self) -> Vec<Vec<String>> {
        self.sections_by_type(SectionType::Asserts)
            .into_iter()
            .filter_map(|s| {
                if let SectionContent::Assertions(asserts) = &s.content {
                    Some(asserts.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get error expected
    #[allow(dead_code)]
    pub fn get_error(&self) -> Option<serde_json::Value> {
        if let Some(section) = self.first_section(SectionType::Error) {
            if let SectionContent::Json(json) = &section.content {
                return Some(json.clone());
            }
        }
        None
    }

    /// Get request headers
    pub fn get_request_headers(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::RequestHeaders) {
            if let SectionContent::KeyValues(headers) = &section.content {
                return Some(headers.clone());
            }
        }
        None
    }

    /// Get TLS configuration
    pub fn get_tls_config(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::Tls) {
            if let SectionContent::KeyValues(config) = &section.content {
                return Some(config.clone());
            }
        }
        None
    }

    /// Get PROTO configuration
    pub fn get_proto_config(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::Proto) {
            if let SectionContent::KeyValues(config) = &section.content {
                return Some(config.clone());
            }
        }
        None
    }

    /// Get OPTIONS configuration
    #[allow(dead_code)]
    pub fn get_options(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::Options) {
            if let SectionContent::KeyValues(options) = &section.content {
                return Some(options.clone());
            }
        }
        None
    }

    /// Check for RESPONSE and ERROR conflict
    pub fn has_response_error_conflict(&self) -> bool {
        self.first_section(SectionType::Response).is_some()
            && self.first_section(SectionType::Error).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_type_from_str() {
        assert_eq!(
            SectionType::from_keyword("ADDRESS"),
            Some(SectionType::Address)
        );
        assert_eq!(
            SectionType::from_keyword("ENDPOINT"),
            Some(SectionType::Endpoint)
        );
        assert_eq!(SectionType::from_keyword("INVALID"), None);
    }

    #[test]
    fn test_section_type_multiple_allowed() {
        assert!(SectionType::Request.is_multiple_allowed());
        assert!(SectionType::Response.is_multiple_allowed());
        assert!(SectionType::Asserts.is_multiple_allowed());
        assert!(!SectionType::Address.is_multiple_allowed());
        assert!(!SectionType::Endpoint.is_multiple_allowed());
    }

    #[test]
    fn test_section_type_supports_inline_options() {
        assert!(SectionType::Response.supports_inline_options());
        assert!(!SectionType::Request.supports_inline_options());
        assert!(!SectionType::Address.supports_inline_options());
    }
}
