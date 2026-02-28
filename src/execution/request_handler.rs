// Request Handler - handles request building and sending

use crate::grpc::GrpcClientConfig;
use crate::parser::ast::{Section, SectionContent, SectionType};
use crate::report::CoverageCollector;
use crate::utils::file::FileUtils;
use prost_reflect::MessageDescriptor;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

/// Request building and sending result
#[derive(Debug, Clone)]
pub struct RequestSendResult {
    pub success: bool,
    pub error_message: Option<String>,
}

/// Request Handler - builds and sends requests
pub struct RequestHandler {
    coverage_collector: Option<Arc<CoverageCollector>>,
}

impl RequestHandler {
    /// Create new request handler
    pub fn new(
        _no_assert: bool,
        _verbose: bool,
        coverage_collector: Option<Arc<CoverageCollector>>,
    ) -> Self {
        Self { coverage_collector }
    }

    /// Build request value from section
    pub fn build_request(
        &self,
        section: &Section,
        variables: &std::collections::HashMap<String, Value>,
    ) -> Option<Value> {
        match &section.content {
            SectionContent::Json(value) => {
                let mut request = value.clone();
                self.substitute_variables(&mut request, variables);
                Some(request)
            }
            SectionContent::JsonLines(_) => {
                // For JSON lines, each line is a separate request
                // This is handled by send_requests_batch
                None
            }
            _ => None,
        }
    }

    /// Send a single request
    pub async fn send_request(
        &self,
        tx: &Sender<Value>,
        request_value: Value,
        section_line: usize,
        _msg_type: Option<&MessageDescriptor>,
    ) -> RequestSendResult {
        // Coverage: record request fields (simplified - would need message type name)
        if let Some(_collector) = &self.coverage_collector {
            // collector.record_fields_from_json(msg_type_name, &request_value);
        }

        match tx.send(request_value).await {
            Ok(_) => RequestSendResult {
                success: true,
                error_message: None,
            },
            Err(e) => RequestSendResult {
                success: false,
                error_message: Some(format!(
                    "Failed to send request at line {}: {}",
                    section_line, e
                )),
            },
        }
    }

    /// Send implicit empty request (for unary/server-stream when no REQUEST section)
    pub async fn send_implicit_empty_request(&self, tx: &Sender<Value>) -> RequestSendResult {
        let empty_request = Value::Object(serde_json::Map::new());

        match tx.send(empty_request).await {
            Ok(_) => RequestSendResult {
                success: true,
                error_message: None,
            },
            Err(e) => RequestSendResult {
                success: false,
                error_message: Some(format!("Failed to send implicit empty request: {}", e)),
            },
        }
    }

    /// Check if request stream should be closed
    pub fn should_close_request_stream(&self, sections: &[Section], current_index: usize) -> bool {
        // Close stream if no more REQUEST sections follow
        sections[current_index + 1..]
            .iter()
            .all(|s| s.section_type != SectionType::Request)
    }

    /// Substitute variables in request value
    pub fn substitute_variables(
        &self,
        value: &mut Value,
        variables: &std::collections::HashMap<String, Value>,
    ) {
        match value {
            Value::String(s) => {
                for (var_name, var_value) in variables {
                    let pattern = format!("{{{{ {} }}}}", var_name);
                    if s.contains(&pattern) {
                        if let Value::String(replacement) = var_value {
                            *s = s.replace(&pattern, replacement);
                        } else {
                            *s = s.replace(&pattern, &var_value.to_string());
                        }
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.substitute_variables(item, variables);
                }
            }
            Value::Object(map) => {
                for (_, val) in map {
                    self.substitute_variables(val, variables);
                }
            }
            _ => {}
        }
    }

    /// Build TLS config from document
    pub fn build_tls_config(
        document: &crate::parser::ast::GctfDocument,
        document_path: &Path,
    ) -> Option<crate::grpc::TlsConfig> {
        document
            .get_tls_config()
            .map(|tls_map| crate::grpc::TlsConfig {
                ca_cert_path: tls_map.get("ca_cert").map(|p| {
                    FileUtils::resolve_relative_path(document_path, p)
                        .to_string_lossy()
                        .to_string()
                }),
                client_cert_path: tls_map.get("client_cert").map(|p| {
                    FileUtils::resolve_relative_path(document_path, p)
                        .to_string_lossy()
                        .to_string()
                }),
                client_key_path: tls_map.get("client_key").map(|p| {
                    FileUtils::resolve_relative_path(document_path, p)
                        .to_string_lossy()
                        .to_string()
                }),
                server_name: tls_map.get("server_name").cloned(),
                insecure_skip_verify: tls_map
                    .get("insecure")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
            })
    }

    /// Build proto config from document
    pub fn build_proto_config(
        document: &crate::parser::ast::GctfDocument,
        document_path: &Path,
    ) -> Option<crate::grpc::ProtoConfig> {
        document.get_proto_config().map(|proto_map| {
            let files = proto_map
                .get("files")
                .map(|f| {
                    f.split(',')
                        .map(|s| {
                            FileUtils::resolve_relative_path(document_path, s.trim())
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let import_paths = proto_map
                .get("import_paths")
                .map(|p| {
                    p.split(',')
                        .map(|s| {
                            FileUtils::resolve_relative_path(document_path, s.trim())
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let descriptor = proto_map.get("descriptor").map(|p| {
                FileUtils::resolve_relative_path(document_path, p)
                    .to_string_lossy()
                    .to_string()
            });

            crate::grpc::ProtoConfig {
                files,
                import_paths,
                descriptor,
            }
        })
    }

    /// Build gRPC client config
    pub fn build_client_config(
        document: &crate::parser::ast::GctfDocument,
        document_path: &Path,
        address: &str,
    ) -> GrpcClientConfig {
        let tls_config = Self::build_tls_config(document, document_path);
        let proto_config = Self::build_proto_config(document, document_path);

        GrpcClientConfig {
            address: address.to_string(),
            timeout_seconds: 30, // Default timeout
            tls_config,
            proto_config,
            metadata: document.get_request_headers(),
            compression: crate::grpc::CompressionMode::from_env(),
            target_service: document.parse_endpoint().map(|(p, s, m)| {
                if p.is_empty() {
                    format!("{}/{}", s, m)
                } else {
                    format!("{}.{}/{}", p, s, m)
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_handler_new() {
        let handler = RequestHandler::new(false, false, None);
        assert!(handler.coverage_collector.is_none());
    }

    #[test]
    fn test_build_request_json() {
        let handler = RequestHandler::new(false, false, None);
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"id": 123})),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };
        let variables = std::collections::HashMap::new();

        let result = handler.build_request(&section, &variables);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), json!({"id": 123}));
    }

    #[test]
    fn test_substitute_variables() {
        let handler = RequestHandler::new(false, false, None);
        let mut value = json!({"id": "{{ user_id }}", "name": "test"});
        let mut variables = std::collections::HashMap::new();
        variables.insert("user_id".to_string(), json!("123"));

        handler.substitute_variables(&mut value, &variables);

        assert_eq!(value["id"], "123");
        assert_eq!(value["name"], "test");
    }

    #[test]
    fn test_should_close_request_stream() {
        let handler = RequestHandler::new(false, false, None);
        let sections = vec![
            Section {
                section_type: SectionType::Request,
                content: SectionContent::Json(json!({})),
                inline_options: Default::default(),
                raw_content: "".to_string(),
                start_line: 0,
                end_line: 0,
            },
            Section {
                section_type: SectionType::Response,
                content: SectionContent::Json(json!({})),
                inline_options: Default::default(),
                raw_content: "".to_string(),
                start_line: 0,
                end_line: 0,
            },
        ];

        // After first section (index 0), there are no more REQUEST sections
        assert!(handler.should_close_request_stream(&sections, 0));
    }

    #[test]
    fn test_build_request_with_variables() {
        let handler = RequestHandler::new(false, false, None);
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"id": "{{ user_id }}"})),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };
        let mut variables = std::collections::HashMap::new();
        variables.insert("user_id".to_string(), json!("456"));

        let result = handler.build_request(&section, &variables);
        assert!(result.is_some());
        assert_eq!(result.unwrap()["id"], "456");
    }
}
