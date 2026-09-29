#[cfg(test)]
#[cfg(test)]
use crate::parser::ast::SectionType;
use crate::parser::ast::{Section, SectionContent};
use crate::report::CoverageCollector;
#[cfg(test)]
use crate::utils::file::FileUtils;
use prost_reflect::MessageDescriptor;
use serde_json::Value;
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub struct RequestSendResult {
    pub success: bool,
    pub error_message: Option<String>,
}

pub struct RequestHandler {
    coverage_collector: Option<Arc<CoverageCollector>>,
}

impl RequestHandler {
    pub fn new(
        _no_assert: bool,
        _verbose: bool,
        coverage_collector: Option<Arc<CoverageCollector>>,
    ) -> Self {
        Self { coverage_collector }
    }

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
            SectionContent::JsonLines(_) => None,
            _ => None,
        }
    }

    pub async fn send_request(
        &self,
        tx: &Sender<Value>,
        request_value: Value,
        section_line: usize,
        _msg_type: Option<&MessageDescriptor>,
    ) -> RequestSendResult {
        if let Some(_collector) = &self.coverage_collector {}

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

    #[cfg(test)]
    pub fn should_close_request_stream(&self, sections: &[Section], current_index: usize) -> bool {
        sections[current_index + 1..]
            .iter()
            .all(|s| s.section_type != SectionType::Request)
    }

    pub fn substitute_variables(
        &self,
        value: &mut Value,
        variables: &std::collections::HashMap<String, Value>,
    ) {
        crate::execution::runner_helpers::substitute_variables(value, variables);
    }

    #[cfg(test)]
    pub fn build_tls_config(
        document: &crate::parser::ast::GctfDocument,
        document_path: &Path,
    ) -> Option<crate::grpc::TlsConfig> {
        document.get_tls_config().map(|tls_map| {
            let path_of = |names: &[&str]| -> Option<String> {
                names.iter().find_map(|n| {
                    tls_map.get(*n).map(|p| {
                        FileUtils::resolve_relative_path(document_path, p)
                            .to_string_lossy()
                            .to_string()
                    })
                })
            };

            crate::grpc::TlsConfig {
                ca_cert_path: path_of(&["ca_cert", "ca_file", "ca"]),
                client_cert_path: path_of(&["client_cert", "cert_file", "cert"]),
                client_key_path: path_of(&["client_key", "key_file", "key"]),
                server_name: tls_map.get("server_name").cloned(),
                insecure_skip_verify: tls_map
                    .get("insecure")
                    .is_some_and(|v| v == "true" || v == "1"),
            }
        })
    }

    #[cfg(test)]
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::SectionSpan;
    use serde_json::json;

    #[test]
    fn request_handler_new() {
        let handler = RequestHandler::new(false, false, None);
        assert!(handler.coverage_collector.is_none());
    }

    #[test]
    fn build_request_json() {
        let handler = RequestHandler::new(false, false, None);
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"id": 123})),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
            span: SectionSpan::default(),
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
                attributes: Vec::new(),
                span: SectionSpan::default(),
            },
            Section {
                section_type: SectionType::Response,
                content: SectionContent::Json(json!({})),
                inline_options: Default::default(),
                raw_content: "".to_string(),
                start_line: 0,
                end_line: 0,
                attributes: Vec::new(),
                span: SectionSpan::default(),
            },
        ];

        assert!(handler.should_close_request_stream(&sections, 0));
    }

    #[test]
    fn build_request_with_variables() {
        let handler = RequestHandler::new(false, false, None);
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"id": "{{ user_id }}"})),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };
        let mut variables = std::collections::HashMap::new();
        variables.insert("user_id".to_string(), json!("456"));

        let result = handler.build_request(&section, &variables);
        assert!(result.is_some());
        assert_eq!(result.unwrap()["id"], "456");
    }
}
