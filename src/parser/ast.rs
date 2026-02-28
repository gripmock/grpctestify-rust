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
    fn current_timestamp() -> i64 {
        #[cfg(miri)]
        {
            0
        }
        #[cfg(not(miri))]
        {
            chrono::Utc::now().timestamp()
        }
    }

    /// Create a new empty document
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            sections: Vec::new(),
            metadata: DocumentMetadata {
                source: None,
                mtime: None,
                parsed_at: Self::current_timestamp(),
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
        if let Some(section) = self.first_section(SectionType::Address)
            && let SectionContent::Single(addr) = &section.content
        {
            return Some(addr.clone());
        }
        env_address.map(|s| s.to_string())
    }

    /// Get endpoint
    pub fn get_endpoint(&self) -> Option<String> {
        if let Some(section) = self.first_section(SectionType::Endpoint)
            && let SectionContent::Single(endpoint) = &section.content
        {
            return Some(endpoint.clone());
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

    /// Get request headers
    pub fn get_request_headers(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::RequestHeaders)
            && let SectionContent::KeyValues(headers) = &section.content
        {
            return Some(headers.clone());
        }
        None
    }

    /// Get TLS configuration
    pub fn get_tls_config(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::Tls)
            && let SectionContent::KeyValues(config) = &section.content
        {
            return Some(config.clone());
        }
        None
    }

    /// Get PROTO configuration
    pub fn get_proto_config(&self) -> Option<HashMap<String, String>> {
        if let Some(section) = self.first_section(SectionType::Proto)
            && let SectionContent::KeyValues(config) = &section.content
        {
            return Some(config.clone());
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
    use serde_json::json;

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

    #[test]
    fn test_section_type_as_str() {
        assert_eq!(SectionType::Address.as_str(), "ADDRESS");
        assert_eq!(SectionType::Endpoint.as_str(), "ENDPOINT");
        assert_eq!(SectionType::Request.as_str(), "REQUEST");
        assert_eq!(SectionType::Response.as_str(), "RESPONSE");
        assert_eq!(SectionType::Error.as_str(), "ERROR");
        assert_eq!(SectionType::RequestHeaders.as_str(), "REQUEST_HEADERS");
        assert_eq!(SectionType::Asserts.as_str(), "ASSERTS");
        assert_eq!(SectionType::Proto.as_str(), "PROTO");
        assert_eq!(SectionType::Tls.as_str(), "TLS");
        assert_eq!(SectionType::Options.as_str(), "OPTIONS");
        assert_eq!(SectionType::Extract.as_str(), "EXTRACT");
    }

    #[test]
    fn test_section_type_from_keyword_aliases() {
        assert_eq!(
            SectionType::from_keyword("HEADERS"),
            Some(SectionType::RequestHeaders)
        );
        assert_eq!(
            SectionType::from_keyword("REQUEST_HEADERS"),
            Some(SectionType::RequestHeaders)
        );
    }

    #[test]
    fn test_section_type_from_keyword_case_insensitive() {
        // Should be case sensitive based on implementation
        assert_eq!(SectionType::from_keyword("address"), None);
        assert_eq!(
            SectionType::from_keyword("  ADDRESS  "),
            Some(SectionType::Address)
        );
    }

    #[test]
    fn test_gctf_document_new() {
        let doc = GctfDocument::new("test.gctf".to_string());
        assert_eq!(doc.file_path, "test.gctf");
        assert!(doc.sections.is_empty());
        assert!(doc.metadata.source.is_none());
        assert!(doc.metadata.mtime.is_none());
    }

    #[test]
    fn test_gctf_document_sections_by_type() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value1"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value2"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 6,
        });

        let requests = doc.sections_by_type(SectionType::Request);
        assert_eq!(requests.len(), 2);

        let responses = doc.sections_by_type(SectionType::Response);
        assert_eq!(responses.len(), 1);

        let errors = doc.sections_by_type(SectionType::Error);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_gctf_document_first_section() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });

        let first_request = doc.first_section(SectionType::Request);
        assert!(first_request.is_some());

        let first_error = doc.first_section(SectionType::Error);
        assert!(first_error.is_none());
    }

    #[test]
    fn test_gctf_document_get_address() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Address,
            content: SectionContent::Single("localhost:4770".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
        });

        assert_eq!(doc.get_address(None), Some("localhost:4770".to_string()));
        assert_eq!(
            doc.get_address(Some("env:5000")),
            Some("localhost:4770".to_string())
        );

        let doc2 = GctfDocument::new("test.gctf".to_string());
        assert_eq!(
            doc2.get_address(Some("env:5000")),
            Some("env:5000".to_string())
        );
        assert_eq!(doc2.get_address(None), None);
    }

    #[test]
    fn test_gctf_document_get_endpoint() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("my.Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
        });

        assert_eq!(doc.get_endpoint(), Some("my.Service/Method".to_string()));

        let doc2 = GctfDocument::new("test.gctf".to_string());
        assert_eq!(doc2.get_endpoint(), None);
    }

    #[test]
    fn test_gctf_document_parse_endpoint() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("package.Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
        });

        let (package, service, method) = doc.parse_endpoint().unwrap();
        assert_eq!(package, "package");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[test]
    fn test_gctf_document_parse_endpoint_no_package() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
        });

        let (package, service, method) = doc.parse_endpoint().unwrap();
        assert_eq!(package, "");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[test]
    fn test_gctf_document_parse_endpoint_invalid() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("invalid".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
        });

        assert!(doc.parse_endpoint().is_none());
    }

    #[test]
    fn test_gctf_document_get_requests() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value1"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value2"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
        });

        let requests = doc.get_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], json!({"key": "value1"}));
        assert_eq!(requests[1], json!({"key": "value2"}));
    }

    #[test]
    fn test_gctf_document_get_assertions() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 1".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".name == \"test\"".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
        });

        let assertions = doc.get_assertions();
        assert_eq!(assertions.len(), 2);
        assert_eq!(assertions[0], vec![".id == 1"]);
        assert_eq!(assertions[1], vec![".name == \"test\""]);
    }

    #[test]
    fn test_gctf_document_get_request_headers() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        doc.sections.push(Section {
            section_type: SectionType::RequestHeaders,
            content: SectionContent::KeyValues(headers.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });

        let result = doc.get_request_headers().unwrap();
        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_gctf_document_get_tls_config() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut config = HashMap::new();
        config.insert("ca_cert".to_string(), "/path/to/ca.pem".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Tls,
            content: SectionContent::KeyValues(config.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });

        let result = doc.get_tls_config().unwrap();
        assert_eq!(result.get("ca_cert"), Some(&"/path/to/ca.pem".to_string()));
    }

    #[test]
    fn test_gctf_document_get_proto_config() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut config = HashMap::new();
        config.insert("files".to_string(), "service.proto".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Proto,
            content: SectionContent::KeyValues(config.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });

        let result = doc.get_proto_config().unwrap();
        assert_eq!(result.get("files"), Some(&"service.proto".to_string()));
    }

    #[test]
    fn test_gctf_document_has_response_error_conflict() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        assert!(!doc.has_response_error_conflict());

        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        });
        assert!(!doc.has_response_error_conflict());

        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
        });
        assert!(doc.has_response_error_conflict());
    }

    #[test]
    fn test_inline_options_default() {
        let options = InlineOptions::default();
        assert!(!options.with_asserts);
        assert!(!options.partial);
        assert!(options.tolerance.is_none());
        assert!(options.redact.is_empty());
        assert!(!options.unordered_arrays);
    }

    #[test]
    fn test_section_content_debug() {
        let content = SectionContent::Single("test".to_string());
        let debug_str = format!("{:?}", content);
        assert!(debug_str.contains("Single"));
    }

    #[test]
    fn test_gctf_document_debug() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let debug_str = format!("{:?}", doc);
        assert!(debug_str.contains("test.gctf"));
    }
}
