// Test runner
// Executes tests defined in GctfDocument
// Refactored to use RequestHandler, ResponseHandler, AssertionHandler

use super::super::parser::GctfDocument;
use super::{AssertionHandler, RequestHandler, ResponseHandler};
use crate::assert::{AssertionEngine, JsonComparator, get_json_diff};
use crate::grpc::{CompressionMode, GrpcClient, GrpcClientConfig, ProtoConfig, TlsConfig};
use crate::optimizer;
use crate::parser::ast::{SectionContent, SectionType};
use crate::plugins::AssertionTiming;
use crate::polyfill::runtime;
use crate::report::CoverageCollector;
use crate::utils::file::FileUtils;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::KeyAndValueRef;

/// Execution plan for inspect workflow visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub file_path: String,
    pub connection: ConnectionInfo,
    pub target: TargetInfo,
    pub headers: Option<HeadersInfo>,
    pub requests: Vec<RequestInfo>,
    pub expectations: Vec<ExpectationInfo>,
    pub assertions: Vec<AssertionInfo>,
    pub extractions: Vec<ExtractionInfo>,
    pub rpc_mode: RpcMode,
    pub summary: ExecutionSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub address: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub endpoint: String,
    pub package: Option<String>,
    pub service: Option<String>,
    pub method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadersInfo {
    pub count: usize,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    pub index: usize,
    pub content: Value,
    pub content_type: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectationInfo {
    pub index: usize,
    pub expectation_type: String, // "response" or "error"
    pub content: Option<Value>,
    pub message_count: Option<usize>,
    pub comparison_options: ComparisonOptions,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComparisonOptions {
    pub partial: bool,
    pub redact: Vec<String>,
    pub tolerance: Option<f64>,
    pub unordered_arrays: bool,
    pub with_asserts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionInfo {
    pub index: usize,
    pub assertions: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionInfo {
    pub index: usize,
    pub variables: HashMap<String, String>,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcMode {
    Unary,
    UnaryError,
    ServerStreaming {
        response_count: usize,
    },
    ClientStreaming {
        request_count: usize,
    },
    BidirectionalStreaming {
        request_count: usize,
        response_count: usize,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionSummary {
    pub total_requests: usize,
    pub total_responses: usize,
    pub total_errors: usize,
    pub error_expected: bool,
    pub assertion_blocks: usize,
    pub variable_extractions: usize,
    pub rpc_mode_name: String,
}

struct AssertionContext<'a> {
    headers: &'a HashMap<String, String>,
    trailers: &'a HashMap<String, String>,
    timing: Option<&'a AssertionTiming>,
}

impl ExecutionPlan {
    /// Build execution plan from a GctfDocument
    pub fn from_document(doc: &GctfDocument) -> Self {
        let file_path = doc.file_path.clone();

        // Connection info
        let connection = if let Some(section) = doc.first_section(SectionType::Address) {
            if let SectionContent::Single(addr) = &section.content {
                ConnectionInfo {
                    address: addr.clone(),
                    source: format!(
                        "ADDRESS section [Line {}-{}]",
                        section.start_line, section.end_line
                    ),
                }
            } else {
                ConnectionInfo {
                    address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                    source: "Environment variable (implicit)".to_string(),
                }
            }
        } else {
            ConnectionInfo {
                address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                source: "Environment variable (implicit)".to_string(),
            }
        };

        // Target info
        let target = if let Some(section) = doc.first_section(SectionType::Endpoint) {
            if let SectionContent::Single(endpoint) = &section.content {
                let (package, service, method) = doc
                    .parse_endpoint()
                    .map(|(p, s, m)| (Some(p), Some(s), Some(m)))
                    .unwrap_or((None, None, None));
                TargetInfo {
                    endpoint: endpoint.clone(),
                    package,
                    service,
                    method,
                }
            } else {
                TargetInfo {
                    endpoint: "<missing>".to_string(),
                    package: None,
                    service: None,
                    method: None,
                }
            }
        } else {
            TargetInfo {
                endpoint: "<missing>".to_string(),
                package: None,
                service: None,
                method: None,
            }
        };

        // Headers info
        let headers = doc
            .first_section(SectionType::RequestHeaders)
            .and_then(|section| {
                if let SectionContent::KeyValues(headers) = &section.content {
                    Some(HeadersInfo {
                        count: headers.len(),
                        headers: headers.clone(),
                    })
                } else {
                    None
                }
            });

        // Requests
        let request_sections = doc.sections_by_type(SectionType::Request);
        let requests: Vec<RequestInfo> = request_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let (content, content_type) = match &section.content {
                    SectionContent::Json(j) => (j.clone(), "json"),
                    SectionContent::JsonLines(_) => (Value::Array(vec![]), "json-lines"),
                    SectionContent::Empty => (Value::Object(serde_json::Map::new()), "empty"),
                    _ => (Value::Null, "unknown"),
                };
                RequestInfo {
                    index: i + 1,
                    content,
                    content_type: content_type.to_string(),
                    line_start: section.start_line,
                    line_end: section.end_line,
                }
            })
            .collect();

        // Expectations (responses or error)
        let response_sections = doc.sections_by_type(SectionType::Response);
        let error_section = doc.first_section(SectionType::Error);

        let expectations: Vec<ExpectationInfo> = if !response_sections.is_empty() {
            response_sections
                .iter()
                .enumerate()
                .map(|(i, section)| {
                    let (content, message_count) = match &section.content {
                        SectionContent::Json(j) => (Some(j.clone()), None),
                        SectionContent::JsonLines(vals) => (None, Some(vals.len())),
                        _ => (None, None),
                    };
                    ExpectationInfo {
                        index: i + 1,
                        expectation_type: "response".to_string(),
                        content,
                        message_count,
                        comparison_options: ComparisonOptions {
                            partial: section.inline_options.partial,
                            redact: section.inline_options.redact.clone(),
                            tolerance: section.inline_options.tolerance,
                            unordered_arrays: section.inline_options.unordered_arrays,
                            with_asserts: section.inline_options.with_asserts,
                        },
                        line_start: section.start_line,
                        line_end: section.end_line,
                    }
                })
                .collect()
        } else if let Some(section) = error_section {
            let content = match &section.content {
                SectionContent::Json(j) => Some(j.clone()),
                _ => None,
            };
            vec![ExpectationInfo {
                index: 1,
                expectation_type: "error".to_string(),
                content,
                message_count: None,
                comparison_options: ComparisonOptions::default(),
                line_start: section.start_line,
                line_end: section.end_line,
            }]
        } else {
            vec![]
        };

        // Assertions
        let assert_sections = doc.sections_by_type(SectionType::Asserts);
        let assertions: Vec<AssertionInfo> = assert_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let assertions = if let SectionContent::Assertions(lines) = &section.content {
                    lines
                        .iter()
                        .map(|line| {
                            optimizer::rewrite_assertion_expression_fixed_point_if_changed(line)
                                .unwrap_or_else(|| line.clone())
                        })
                        .collect()
                } else {
                    vec![]
                };
                AssertionInfo {
                    index: i + 1,
                    assertions,
                    line_start: section.start_line,
                    line_end: section.end_line,
                }
            })
            .collect();

        // Extractions
        let extract_sections = doc.sections_by_type(SectionType::Extract);
        let extractions: Vec<ExtractionInfo> = extract_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let variables = if let SectionContent::Extract(vars) = &section.content {
                    vars.clone()
                } else {
                    HashMap::new()
                };
                ExtractionInfo {
                    index: i + 1,
                    variables,
                    line_start: section.start_line,
                    line_end: section.end_line,
                }
            })
            .collect();

        // Infer RPC mode
        let has_json_lines = response_sections
            .iter()
            .any(|s| matches!(&s.content, SectionContent::JsonLines(vals) if vals.len() > 1));
        let rpc_mode = infer_rpc_mode(
            &requests,
            &expectations,
            error_section.is_some(),
            has_json_lines,
        );

        // Summary
        let rpc_mode_name = match &rpc_mode {
            RpcMode::Unary => "Unary",
            RpcMode::UnaryError => "Unary Error",
            RpcMode::ServerStreaming { .. } => "Server Streaming",
            RpcMode::ClientStreaming { .. } => "Client Streaming",
            RpcMode::BidirectionalStreaming { .. } => "Bidirectional Streaming",
            RpcMode::Unknown => "Unknown",
        };

        let summary = ExecutionSummary {
            total_requests: requests.len(),
            total_responses: expectations
                .iter()
                .filter(|e| e.expectation_type == "response")
                .count(),
            total_errors: expectations
                .iter()
                .filter(|e| e.expectation_type == "error")
                .count(),
            error_expected: expectations.iter().any(|e| e.expectation_type == "error"),
            assertion_blocks: assertions.len(),
            variable_extractions: extractions.len(),
            rpc_mode_name: rpc_mode_name.to_string(),
        };

        ExecutionPlan {
            file_path,
            connection,
            target,
            headers,
            requests,
            expectations,
            assertions,
            extractions,
            rpc_mode,
            summary,
        }
    }
}

fn infer_rpc_mode(
    requests: &[RequestInfo],
    expectations: &[ExpectationInfo],
    has_error: bool,
    has_json_lines: bool,
) -> RpcMode {
    let req_count = requests.len();
    let resp_count = expectations
        .iter()
        .filter(|e| e.expectation_type == "response")
        .count();

    if has_error {
        RpcMode::UnaryError
    } else if has_json_lines || resp_count > 1 {
        if req_count > 1 {
            RpcMode::BidirectionalStreaming {
                request_count: req_count,
                response_count: resp_count,
            }
        } else {
            RpcMode::ServerStreaming {
                response_count: resp_count,
            }
        }
    } else if req_count > 1 {
        RpcMode::ClientStreaming {
            request_count: req_count,
        }
    } else if req_count == 1 && resp_count == 1 {
        RpcMode::Unary
    } else if req_count == 0 && resp_count > 0 {
        RpcMode::ServerStreaming {
            response_count: resp_count,
        }
    } else {
        RpcMode::Unknown
    }
}

/// Test execution status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExecutionStatus {
    Pass,
    Fail(String),
}

/// Test execution result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestExecutionResult {
    pub status: TestExecutionStatus,
    pub grpc_duration_ms: Option<u64>,
    // Optional: captured response for updating the test file
    pub captured_response: Option<crate::grpc::GrpcResponse>,
}

#[derive(Debug, Default, Clone)]
struct AssertionScopeTimingState {
    last_message_elapsed_ms: Option<u64>,
    total_scope_elapsed_ms: u64,
    scope_index: usize,
}

impl AssertionScopeTimingState {
    fn finish_scope(
        &mut self,
        scope_start_ms: u64,
        scope_end_ms: u64,
        scope_message_count: usize,
    ) -> Option<AssertionTiming> {
        if scope_message_count == 0 {
            return None;
        }

        let elapsed_ms = scope_end_ms.saturating_sub(scope_start_ms);
        self.scope_index += 1;
        self.total_scope_elapsed_ms = self.total_scope_elapsed_ms.saturating_add(elapsed_ms);

        let timing = AssertionTiming {
            elapsed_ms,
            total_elapsed_ms: self.total_scope_elapsed_ms,
            scope_message_count,
            scope_index: self.scope_index,
        };

        Some(timing)
    }
}

impl TestExecutionResult {
    pub fn pass(grpc_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Pass,
            grpc_duration_ms,
            captured_response: None,
        }
    }

    pub fn fail(message: String, grpc_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Fail(message),
            grpc_duration_ms,
            captured_response: None,
        }
    }

    pub fn with_response(mut self, response: crate::grpc::GrpcResponse) -> Self {
        self.captured_response = Some(response);
        self
    }
}

/// Test runner
pub struct TestRunner {
    dry_run: bool,
    timeout_seconds: u64,
    no_assert: bool,
    write_mode: bool,
    verbose: bool,
    assertion_engine: AssertionEngine,
    coverage_collector: Option<Arc<CoverageCollector>>,
    // Handler modules for delegated functionality
    request_handler: RequestHandler,
    response_handler: ResponseHandler,
    assertion_handler: AssertionHandler,
}

fn parse_bool_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

fn tls_env_defaults() -> HashMap<String, String> {
    let mut defaults = HashMap::new();

    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("ca_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("client_cert".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE)
        && !value.trim().is_empty()
    {
        defaults.insert("client_key".to_string(), value);
    }
    if let Ok(value) = std::env::var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME)
        && !value.trim().is_empty()
    {
        defaults.insert("server_name".to_string(), value);
    }

    defaults
}

fn resolve_tls_path(value: &str, from_env: bool, document_path: &Path) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_string_lossy().to_string();
    }

    if from_env {
        if runtime::supports(runtime::Capability::IsolatedFsIo)
            && let Ok(cwd) = std::env::current_dir()
        {
            return cwd.join(path).to_string_lossy().to_string();
        }
        return path.to_string_lossy().to_string();
    }

    FileUtils::resolve_relative_path(document_path, value)
        .to_string_lossy()
        .to_string()
}

impl TestRunner {
    pub fn full_service_name(package: &str, service: &str) -> String {
        if package.is_empty() {
            service.to_string()
        } else {
            format!("{}.{}", package, service)
        }
    }

    fn format_json_pretty(value: &Value) -> String {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    }

    fn interpolate_variables(template: &str, variables: &HashMap<String, Value>) -> Option<String> {
        let mut out = String::with_capacity(template.len());
        let mut cursor = 0usize;
        let mut changed = false;

        while let Some(open_rel) = template[cursor..].find("{{") {
            let open = cursor + open_rel;
            out.push_str(&template[cursor..open]);

            let after_open = open + 2;
            let Some(close_rel) = template[after_open..].find("}}") else {
                out.push_str(&template[open..]);
                return changed.then_some(out);
            };
            let close = after_open + close_rel;

            let key = template[after_open..close].trim();
            if let Some(val) = variables.get(key) {
                match val {
                    Value::String(s) => out.push_str(s),
                    _ => out.push_str(&val.to_string()),
                }
                changed = true;
            } else {
                out.push_str(&template[open..close + 2]);
            }

            cursor = close + 2;
        }

        if !changed {
            return None;
        }

        out.push_str(&template[cursor..]);
        Some(out)
    }

    pub fn expected_values_for_response_section(
        section: &crate::parser::ast::Section,
    ) -> Vec<Value> {
        match &section.content {
            SectionContent::Json(v) => vec![v.clone()],
            SectionContent::JsonLines(values) => values.clone(),
            _ => Vec::new(),
        }
    }

    pub fn grpc_code_name_from_numeric(code: i64) -> Option<&'static str> {
        super::error_handler::ErrorHandler::grpc_code_name_from_numeric(code)
    }

    pub fn error_matches_expected(error_text: &str, expected: &Value) -> bool {
        super::error_handler::ErrorHandler::error_matches_expected(error_text, expected)
    }

    /// Create a new test runner
    pub fn new(
        dry_run: bool,
        timeout_seconds: u64,
        no_assert: bool,
        write_mode: bool,
        verbose: bool,
        coverage_collector: Option<Arc<CoverageCollector>>,
    ) -> Self {
        Self {
            dry_run,
            timeout_seconds,
            no_assert,
            write_mode,
            verbose,
            assertion_engine: AssertionEngine::new(),
            coverage_collector: coverage_collector.clone(),
            // Initialize handler modules
            request_handler: RequestHandler::new(no_assert, verbose, coverage_collector.clone()),
            response_handler: ResponseHandler::new(no_assert),
            assertion_handler: AssertionHandler::new(verbose),
        }
    }

    /// Run a single test
    pub async fn run_test(&self, document: &GctfDocument) -> Result<TestExecutionResult> {
        let effective_dry_run = self.dry_run;
        let effective_no_assert = self.no_assert;
        let effective_write_mode = self.write_mode;

        let options = document.get_options().unwrap_or_default();
        let effective_timeout_seconds = match options.get("timeout") {
            Some(value) => match value.trim().parse::<u64>() {
                Ok(v) if v > 0 => v,
                _ => {
                    return Ok(TestExecutionResult::fail(
                        format!(
                            "OPTIONS.timeout must be a positive integer, got '{}'",
                            value
                        ),
                        None,
                    ));
                }
            },
            None => self.timeout_seconds,
        };

        // Validate file path in update mode
        if effective_write_mode {
            let file_path = Path::new(&document.file_path);
            if !file_path.exists() {
                return Ok(TestExecutionResult::fail(
                    format!("Update mode: file '{}' does not exist", document.file_path),
                    None,
                ));
            }

            // Check if file is writable
            use std::fs::OpenOptions;
            if OpenOptions::new().write(true).open(file_path).is_err() {
                return Ok(TestExecutionResult::fail(
                    format!("Update mode: file '{}' is not writable", document.file_path),
                    None,
                ));
            }
        }

        // Extract address
        let address = match document.get_address(
            std::env::var(crate::config::ENV_GRPCTESTIFY_ADDRESS)
                .ok()
                .as_deref(),
        ) {
            Some(a) => a,
            None => {
                // Default to localhost:4770 if no address is specified anywhere
                crate::config::default_address()
            }
        };

        // Extract endpoint
        let (package, service, method) = match document.parse_endpoint() {
            Some(e) => e,
            None => {
                return Ok(TestExecutionResult::fail(
                    "Invalid or missing endpoint".to_string(),
                    None,
                ));
            }
        };

        if document.sections.is_empty() {
            return Ok(TestExecutionResult::fail(
                "No sections found".to_string(),
                None,
            ));
        }

        if effective_dry_run {
            // In dry-run, show detailed preview of what will be executed
            self.print_dry_run_preview(document, &address, &package, &service, &method);
            return Ok(TestExecutionResult::pass(None));
        }

        // Configure Client
        let document_path = Path::new(&document.file_path);

        let tls_defaults = tls_env_defaults();
        let tls_section = document.get_tls_config();

        let pick_tls_value = |keys: &[&str]| -> Option<(String, bool)> {
            if let Some(section_map) = tls_section.as_ref() {
                for key in keys {
                    if let Some(value) = section_map.get(*key) {
                        return Some((value.clone(), false));
                    }
                }
            }

            for key in keys {
                if let Some(value) = tls_defaults.get(*key) {
                    return Some((value.clone(), true));
                }
            }

            None
        };

        let ca_cert_path = pick_tls_value(&["ca_cert", "ca_file"])
            .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
        let client_cert_path = pick_tls_value(&["client_cert", "cert", "cert_file"])
            .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
        let client_key_path = pick_tls_value(&["client_key", "key", "key_file"])
            .map(|(v, from_env)| resolve_tls_path(&v, from_env, document_path));
        let server_name = pick_tls_value(&["server_name"]).map(|(v, _)| v);
        let insecure_skip_verify = tls_section
            .as_ref()
            .and_then(|m| m.get("insecure"))
            .map(|s| parse_bool_flag(s))
            .unwrap_or(false);

        let tls_config = if ca_cert_path.is_some()
            || client_cert_path.is_some()
            || client_key_path.is_some()
            || server_name.is_some()
            || insecure_skip_verify
        {
            Some(TlsConfig {
                ca_cert_path,
                client_cert_path,
                client_key_path,
                server_name,
                insecure_skip_verify,
            })
        } else {
            None
        };

        // Check for Proto config in document
        let proto_config = if let Some(proto_map) = document.get_proto_config() {
            let files = proto_map
                .get("files")
                .map(|s| {
                    s.split(',')
                        .map(|p| {
                            FileUtils::resolve_relative_path(document_path, p.trim())
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let import_paths = proto_map
                .get("import_paths")
                .map(|s| {
                    s.split(',')
                        .map(|p| {
                            FileUtils::resolve_relative_path(document_path, p.trim())
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

            Some(ProtoConfig {
                files,
                import_paths,
                descriptor,
            })
        } else {
            None
        };

        let full_service = Self::full_service_name(&package, &service);

        let client_config = GrpcClientConfig {
            address,
            timeout_seconds: effective_timeout_seconds,
            tls_config,
            proto_config,
            metadata: document.get_request_headers(),
            target_service: Some(full_service.clone()),
            compression: CompressionMode::from_env(),
        };

        let client = GrpcClient::new(client_config).await?;

        // Get input/output message types for field coverage tracking
        let input_message_type = client
            .descriptor_pool()
            .get_service_by_name(&full_service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .map(|m| m.input().full_name().to_string());
        let output_message_type = client
            .descriptor_pool()
            .get_service_by_name(&full_service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .map(|m| m.output().full_name().to_string());

        // Setup Streaming
        let (tx, rx) = mpsc::channel::<Value>(100);
        let request_stream = ReceiverStream::new(rx);
        let mut tx = Some(tx);

        // Coverage: Register pool and record call
        if let Some(collector) = &self.coverage_collector {
            collector.register_pool(client.descriptor_pool());
            collector.record_call(&full_service, &method);
        }

        let start_time = std::time::Instant::now();

        // Start the gRPC call in background so unary/server-streaming methods can wait
        // for the first request message without deadlocking this task.
        let full_service_clone = full_service.clone();
        let method_clone = method.clone();
        let mut client_for_call = client;
        let mut call_handle = Some(tokio::spawn(async move {
            client_for_call
                .call_stream(&full_service_clone, &method_clone, request_stream)
                .await
        }));

        let mut response_stream = None;

        let mut variables: HashMap<String, Value> = HashMap::new();
        let mut last_message: Option<Value> = None;
        let mut last_error_message: Option<String> = None;
        let mut last_error_timing: Option<AssertionTiming> = None;
        let mut captured_headers: HashMap<String, String> = HashMap::new();
        let mut captured_trailers: HashMap<String, String> = HashMap::new();
        let mut failure_reasons: Vec<String> = Vec::new();
        let mut assertion_timing = AssertionScopeTimingState::default();

        // Iterator for sections
        // We iterate by index to allow lookahead
        let sections = &document.sections;

        let has_request_sections = sections
            .iter()
            .any(|s| s.section_type == SectionType::Request);

        // Legacy behavior: if no REQUEST section is provided, send an empty
        // JSON object as a single request message for unary/server-stream calls.
        if !has_request_sections && let Some(tx_ref) = tx.as_mut() {
            if let Err(e) = tx_ref.send(Value::Object(serde_json::Map::new())).await {
                failure_reasons.push(format!("Failed to send implicit empty request: {}", e));
            }
            drop(tx.take());
        }

        let mut skip_next_section = false;

        // Capture full response for write mode
        let mut captured_response = if effective_write_mode {
            Some(crate::grpc::GrpcResponse::new())
        } else {
            None
        };

        for (i, section) in sections.iter().enumerate() {
            if skip_next_section {
                skip_next_section = false;
                continue;
            }

            match section.section_type {
                SectionType::Request => {
                    // Build request using RequestHandler
                    let request_value = match &section.content {
                        SectionContent::Json(req_json) => {
                            let mut req = req_json.clone();
                            self.substitute_variables(&mut req, &variables);
                            req
                        }
                        SectionContent::Empty => Value::Object(serde_json::Map::new()),
                        _ => continue,
                    };

                    // Coverage: record request fields
                    if let (Some(collector), Some(msg_type)) =
                        (&self.coverage_collector, &input_message_type)
                    {
                        collector.record_fields_from_json(msg_type, &request_value);
                    }

                    // Send request using RequestHandler
                    let Some(tx_ref) = tx.as_mut() else {
                        failure_reasons.push(format!(
                            "Failed to send request at line {}: request stream already closed",
                            section.start_line
                        ));
                        break;
                    };

                    let result = self
                        .request_handler
                        .send_request(tx_ref, request_value, section.start_line, None)
                        .await;
                    if !result.success
                        && let Some(error) = result.error_message
                    {
                        failure_reasons.push(error);
                    }
                }
                SectionType::Response => {
                    let scope_start_ms = assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                    let mut scope_end_ms = scope_start_ms;
                    let mut scope_message_count = 0usize;

                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none()
                        && let Some(handle) = call_handle.take()
                    {
                        match handle.await {
                            Ok(Ok((h, stream))) => {
                                if let Some(resp) = &mut captured_response {
                                    captured_headers = h.clone();
                                    resp.headers = h;
                                } else {
                                    captured_headers = h;
                                }
                                response_stream = Some(stream);
                            }
                            Ok(Err(e)) => {
                                failure_reasons.push(format!("Failed to start gRPC stream: {}", e));
                                break;
                            }
                            Err(e) => {
                                failure_reasons.push(format!(
                                    "Failed to join gRPC stream startup task: {}",
                                    e
                                ));
                                break;
                            }
                        }
                    }

                    let mut received_messages_for_section: Vec<Value> = Vec::new();
                    let expected_values = Self::expected_values_for_response_section(section);

                    for expected_template in expected_values {
                        match response_stream.as_mut().unwrap().next().await {
                            Some(Ok(item)) => {
                                match item {
                                    crate::grpc::client::StreamItem::Message(msg) => {
                                        let now_elapsed_ms =
                                            start_time.elapsed().as_millis() as u64;

                                        let msg_for_state = msg.clone();
                                        last_message = Some(msg_for_state.clone());
                                        if section.inline_options.with_asserts {
                                            received_messages_for_section
                                                .push(msg_for_state.clone());
                                        }
                                        scope_end_ms = now_elapsed_ms;
                                        scope_message_count += 1;
                                        assertion_timing.last_message_elapsed_ms =
                                            Some(now_elapsed_ms);
                                        if let Some(resp) = &mut captured_response {
                                            resp.messages.push(msg_for_state);
                                        }

                                        let should_format_message =
                                            tracing::enabled!(tracing::Level::DEBUG)
                                                || effective_no_assert
                                                || self.verbose;
                                        let msg_pretty = should_format_message
                                            .then(|| Self::format_json_pretty(&msg));

                                        if let Some(pretty) = msg_pretty.as_deref()
                                            && tracing::enabled!(tracing::Level::DEBUG)
                                        {
                                            tracing::debug!("Received Response:\n{}", pretty);
                                        }

                                        if effective_no_assert {
                                            println!("--- RESPONSE (Raw) ---");
                                            if let Some(pretty) = msg_pretty.as_deref() {
                                                println!("{}", pretty);
                                            }
                                        } else if self.verbose {
                                            if let Some(pretty) = msg_pretty.as_deref() {
                                                println!("🔍 gRPC response received: '{}'", pretty);
                                            }
                                        }

                                        if !effective_no_assert {
                                            let mut expected = expected_template.clone();
                                            self.substitute_variables(&mut expected, &variables);

                                            // Coverage: record expected response fields
                                            if let (Some(collector), Some(msg_type)) =
                                                (&self.coverage_collector, &output_message_type)
                                            {
                                                collector
                                                    .record_fields_from_json(msg_type, &expected);
                                            }

                                            let diffs = JsonComparator::compare(
                                                &msg,
                                                &expected,
                                                &section.inline_options,
                                            );

                                            if !diffs.is_empty() {
                                                failure_reasons.push(format!(
                                                    "Response mismatch at line {}:",
                                                    section.start_line
                                                ));
                                                for diff in diffs {
                                                    match diff {
                                                        crate::assert::AssertionResult::Fail {
                                                            message,
                                                            expected,
                                                            actual,
                                                        } => {
                                                            let mut msg =
                                                                format!("  - {}", message);
                                                            if let (Some(exp), Some(act)) =
                                                                (expected, actual)
                                                            {
                                                                msg.push_str(&format!("\n      Expected: {}\n      Actual:   {}", exp, act));
                                                            }
                                                            failure_reasons.push(msg);
                                                        }
                                                        crate::assert::AssertionResult::Error(
                                                            m,
                                                        ) => failure_reasons
                                                            .push(format!("  - Error: {}", m)),
                                                        _ => {}
                                                    }
                                                }
                                                failure_reasons
                                                    .push(get_json_diff(&expected, &msg));
                                            }
                                        }
                                    }
                                    crate::grpc::client::StreamItem::Trailers(t) => {
                                        if let Some(resp) = &mut captured_response {
                                            captured_trailers.extend(
                                                t.iter().map(|(k, v)| (k.clone(), v.clone())),
                                            );
                                            resp.trailers.extend(t);
                                        } else {
                                            captured_trailers.extend(t);
                                        }
                                        if !effective_no_assert {
                                            failure_reasons.push(format!(
                                                "Expected message for RESPONSE section at line {}, but received Trailers (End of Stream)",
                                                section.start_line
                                            ));
                                        }
                                        break;
                                    }
                                }
                            }
                            Some(Err(status)) => {
                                let scope_start_ms =
                                    assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                                let scope_end_ms = start_time.elapsed().as_millis() as u64;
                                assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                                last_error_timing =
                                    assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);
                                last_error_message = Some(status.message().to_string());

                                if let Some(resp) = &mut captured_response {
                                    resp.error = Some(status.message().to_string());
                                }
                                if !effective_no_assert {
                                    failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but received Error: {}",
                                        section.start_line,
                                        status.message()
                                    ));
                                } else {
                                    println!("--- RESPONSE (Error) ---");
                                    println!("{}", status.message());
                                }
                                break;
                            }
                            None => {
                                if !effective_no_assert {
                                    failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but stream ended",
                                        section.start_line
                                    ));
                                }
                                break;
                            }
                        }
                    }

                    if section.inline_options.with_asserts
                        && let Some(next_section) = sections.get(i + 1)
                        && next_section.section_type == SectionType::Asserts
                    {
                        if !effective_no_assert
                            && let SectionContent::Assertions(lines) = &next_section.content
                        {
                            let scope_timing = assertion_timing.finish_scope(
                                scope_start_ms,
                                scope_end_ms,
                                scope_message_count,
                            );

                            for msg in &received_messages_for_section {
                                self.run_assertions(
                                    lines,
                                    msg,
                                    &mut failure_reasons,
                                    section.start_line,
                                    AssertionContext {
                                        headers: &captured_headers,
                                        trailers: &captured_trailers,
                                        timing: scope_timing.as_ref(),
                                    },
                                );
                            }
                        }
                        skip_next_section = true;
                    } else if section.inline_options.with_asserts && !effective_no_assert {
                        failure_reasons.push(format!(
                            "RESPONSE at line {} has 'with_asserts' but is not followed by ASSERTS",
                            section.start_line
                        ));
                    }
                }
                SectionType::Asserts => {
                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none()
                        && let Some(handle) = call_handle.take()
                    {
                        match handle.await {
                            Ok(Ok((h, stream))) => {
                                if let Some(resp) = &mut captured_response {
                                    captured_headers = h.clone();
                                    resp.headers = h;
                                } else {
                                    captured_headers = h;
                                }
                                response_stream = Some(stream);
                            }
                            Ok(Err(e)) => {
                                failure_reasons.push(format!("Failed to start gRPC stream: {}", e));
                                break;
                            }
                            Err(e) => {
                                failure_reasons.push(format!(
                                    "Failed to join gRPC stream startup task: {}",
                                    e
                                ));
                                break;
                            }
                        }
                    }

                    // Standalone ASSERTS usually consumes a new message.
                    // If stream is unavailable but we already captured an ERROR,
                    // evaluate assertions against that error context.
                    let Some(stream) = response_stream.as_mut() else {
                        if !effective_no_assert
                            && let SectionContent::Assertions(lines) = &section.content
                        {
                            if let Some(error_message) = &last_error_message {
                                let error_value = Value::String(error_message.clone());
                                self.run_assertions(
                                    lines,
                                    &error_value,
                                    &mut failure_reasons,
                                    section.start_line,
                                    AssertionContext {
                                        headers: &captured_headers,
                                        trailers: &captured_trailers,
                                        timing: last_error_timing.as_ref(),
                                    },
                                );
                            } else {
                                failure_reasons.push(format!(
                                    "ASSERTS section at line {} has no active response/error context",
                                    section.start_line
                                ));
                            }
                        }
                        continue;
                    };

                    match stream.next().await {
                        Some(Ok(crate::grpc::client::StreamItem::Message(msg))) => {
                            let scope_start_ms =
                                assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                            let scope_end_ms = start_time.elapsed().as_millis() as u64;
                            assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                            let scope_timing =
                                assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);

                            last_message = Some(msg.clone());

                            let should_format_message =
                                tracing::enabled!(tracing::Level::DEBUG) || effective_no_assert;
                            let msg_pretty =
                                should_format_message.then(|| Self::format_json_pretty(&msg));

                            if let Some(pretty) = msg_pretty.as_deref()
                                && tracing::enabled!(tracing::Level::DEBUG)
                            {
                                tracing::debug!("Received Response (for Asserts):\n{}", pretty);
                            }

                            if effective_no_assert {
                                println!("--- RESPONSE (Raw) ---");
                                if let Some(pretty) = msg_pretty.as_deref() {
                                    println!("{}", pretty);
                                }
                            }

                            if !effective_no_assert
                                && let SectionContent::Assertions(lines) = &section.content
                            {
                                self.run_assertions(
                                    lines,
                                    &msg,
                                    &mut failure_reasons,
                                    section.start_line,
                                    AssertionContext {
                                        headers: &captured_headers,
                                        trailers: &captured_trailers,
                                        timing: scope_timing.as_ref(),
                                    },
                                );
                            }
                        }
                        Some(Ok(crate::grpc::client::StreamItem::Trailers(t))) => {
                            captured_trailers.extend(t);
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                    "Expected message for ASSERTS section at line {}, but received Trailers",
                                    section.start_line
                                ));
                            }
                        }
                        Some(Err(status)) => {
                            let scope_start_ms =
                                assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                            let scope_end_ms = start_time.elapsed().as_millis() as u64;
                            assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                            last_error_timing =
                                assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);

                            last_error_message = Some(status.message().to_string());
                            captured_trailers
                                .extend(Self::metadata_map_to_hashmap(status.metadata()));
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                     "Expected message for ASSERTS section at line {}, but received Error: {}",
                                     section.start_line, status.message()
                                 ));
                            } else {
                                println!("--- RESPONSE (Error) ---");
                                println!("{}", status.message());
                            }
                        }
                        None => {
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                    "Expected message for ASSERTS section at line {}, but stream ended",
                                    section.start_line
                                ));
                            }
                        }
                    }
                }

                SectionType::Extract => {
                    if let Some(msg) = &last_message {
                        if let SectionContent::Extract(extractions) = &section.content {
                            for (key, query) in extractions {
                                match self.assertion_engine.query(query, msg) {
                                    Ok(results) => {
                                        if let Some(val) = results.first() {
                                            variables.insert(key.clone(), val.clone());
                                        } else {
                                            failure_reasons.push(format!(
                                                 "Extraction failed at line {}: Query '{}' returned no results",
                                                 section.start_line, query
                                             ));
                                        }
                                    }
                                    Err(e) => {
                                        failure_reasons.push(format!(
                                            "Extraction error at line {}: {}",
                                            section.start_line, e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        failure_reasons.push(format!(
                            "EXTRACT at line {} requires a previous response message",
                            section.start_line
                        ));
                    }
                }
                SectionType::Error => {
                    if sections[i + 1..]
                        .iter()
                        .all(|s| s.section_type != SectionType::Request)
                    {
                        drop(tx.take());
                    }

                    if response_stream.is_none()
                        && let Some(handle) = call_handle.take()
                    {
                        match handle.await {
                            Ok(Ok((h, stream))) => {
                                if let Some(resp) = &mut captured_response {
                                    captured_headers = h.clone();
                                    resp.headers = h;
                                } else {
                                    captured_headers = h;
                                }
                                response_stream = Some(stream);
                            }
                            Ok(Err(e)) => {
                                // If ERROR section is expected, startup failures from unary/client-streaming
                                // calls may represent the expected application error.
                                if !effective_no_assert {
                                    if let SectionContent::Json(expected_json) = &section.content {
                                        let mut expected = expected_json.clone();
                                        self.substitute_variables(&mut expected, &variables);

                                        // Try to extract tonic Status from anyhow::Error
                                        let (matches, got, mismatch_reason) = if let Some(status) =
                                            e.downcast_ref::<tonic::Status>()
                                        {
                                            last_error_message = Some(status.message().to_string());
                                            captured_trailers.extend(
                                                Self::metadata_map_to_hashmap(status.metadata()),
                                            );
                                            let status_name = Self::grpc_code_name_from_numeric(
                                                status.code() as i64,
                                            )
                                            .unwrap_or("Unknown");
                                            (
                                                super::error_handler::ErrorHandler::status_matches_expected(
                                                    status,
                                                    &expected,
                                                ),
                                                format!(
                                                    "status: {}, message: \"{}\"",
                                                    status_name,
                                                    status.message()
                                                ),
                                                super::error_handler::ErrorHandler::status_mismatch_reason(
                                                    status,
                                                    &expected,
                                                ),
                                            )
                                        } else {
                                            // Fallback to error string representation
                                            let text = e.to_string();
                                            (
                                                Self::error_matches_expected(&text, &expected),
                                                text,
                                                None,
                                            )
                                        };

                                        if self.verbose {
                                            println!("🔍 gRPC error received: '{}'", got);
                                            if let Some(status) = e.downcast_ref::<tonic::Status>()
                                            {
                                                let details_json = super::error_handler::ErrorHandler::status_details_json(status);
                                                if details_json != Value::Null
                                                    && details_json
                                                        .as_array()
                                                        .is_some_and(|arr| !arr.is_empty())
                                                {
                                                    println!(
                                                        "🔍 gRPC error details: {}",
                                                        details_json
                                                    );
                                                }
                                            }
                                        }

                                        if !matches {
                                            failure_reasons.push(format!(
                                                "Error mismatch at line {}:",
                                                section.start_line
                                            ));
                                            if let Some(reason) = mismatch_reason {
                                                failure_reasons.push(format!("  - {}", reason));
                                            }
                                            if let Some(status) = e.downcast_ref::<tonic::Status>()
                                            {
                                                let actual_json =
                                                    super::error_handler::ErrorHandler::status_to_json(
                                                        status,
                                                    );
                                                failure_reasons
                                                    .push(get_json_diff(&expected, &actual_json));
                                            } else {
                                                failure_reasons.push(format!(
                                                    "  - expected {}, got '{}'",
                                                    expected, got
                                                ));
                                            }
                                        }
                                    }
                                } else {
                                    println!("--- RESPONSE (Error) ---");
                                    println!("{}", e);
                                }
                                // Error has been consumed at startup stage; continue with next sections.
                                continue;
                            }
                            Err(e) => {
                                failure_reasons.push(format!(
                                    "Failed to join gRPC stream startup task: {}",
                                    e
                                ));
                                break;
                            }
                        }
                    }

                    // Expect an error from the stream
                    match response_stream.as_mut().unwrap().next().await {
                        Some(Err(status)) => {
                            let scope_start_ms =
                                assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                            let scope_end_ms = start_time.elapsed().as_millis() as u64;
                            assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                            let error_scope_timing =
                                assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);

                            let status_message = status.message();
                            last_error_message = Some(status_message.to_string());
                            last_error_timing = error_scope_timing;
                            captured_trailers
                                .extend(Self::metadata_map_to_hashmap(status.metadata()));
                            let should_format_error = effective_no_assert || self.verbose;
                            let got = should_format_error.then(|| {
                                let status_name =
                                    Self::grpc_code_name_from_numeric(status.code() as i64)
                                        .unwrap_or("Unknown");
                                format!("status: {}, message: \"{}\"", status_name, status_message)
                            });

                            if effective_no_assert {
                                println!("--- RESPONSE (Error) ---");
                                if let Some(got) = got.as_deref() {
                                    println!("{}", got);
                                }
                            } else if self.verbose {
                                if let Some(got) = got.as_deref() {
                                    println!("🔍 gRPC error received: '{}'", got);
                                }
                                let details_json =
                                    super::error_handler::ErrorHandler::status_details_json(
                                        &status,
                                    );
                                if details_json != Value::Null
                                    && details_json.as_array().is_some_and(|arr| !arr.is_empty())
                                {
                                    println!("🔍 gRPC error details: {}", details_json);
                                }
                            }

                            if !effective_no_assert {
                                if let SectionContent::Json(expected_json) = &section.content {
                                    let mut expected = expected_json.clone();
                                    self.substitute_variables(&mut expected, &variables);

                                    if !super::error_handler::ErrorHandler::status_matches_expected(
                                        &status, &expected,
                                    ) {
                                        failure_reasons.push(format!(
                                            "Error mismatch at line {}:",
                                            section.start_line
                                        ));
                                        if let Some(reason) =
                                            super::error_handler::ErrorHandler::status_mismatch_reason(
                                                &status,
                                                &expected,
                                            )
                                        {
                                            failure_reasons.push(format!("  - {}", reason));
                                        }
                                        let actual_json =
                                            super::error_handler::ErrorHandler::status_to_json(
                                                &status,
                                            );
                                        failure_reasons
                                            .push(get_json_diff(&expected, &actual_json));
                                    }
                                }

                                // Handle with_asserts for Error
                                if section.inline_options.with_asserts
                                    && let Some(next_section) = sections.get(i + 1)
                                    && next_section.section_type == SectionType::Asserts
                                    && let SectionContent::Assertions(lines) = &next_section.content
                                {
                                    let error_value = Value::String(status.message().to_string());
                                    self.run_assertions(
                                        lines,
                                        &error_value,
                                        &mut failure_reasons,
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: last_error_timing.as_ref(),
                                        },
                                    );
                                    skip_next_section = true;
                                }
                            } else {
                                // In no_assert mode, we still need to skip the attached ASSERTS section if present
                                if section.inline_options.with_asserts
                                    && let Some(next_section) = sections.get(i + 1)
                                    && next_section.section_type == SectionType::Asserts
                                {
                                    skip_next_section = true;
                                }
                            }
                        }
                        Some(Ok(msg_item)) => {
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                    "Expected ERROR at line {}, but received success message or trailers",
                                    section.start_line
                                ));
                            } else {
                                // If we got a message instead of error in no_assert mode, print it
                                if let crate::grpc::client::StreamItem::Message(msg) = msg_item {
                                    println!("--- RESPONSE (Raw) ---");
                                    println!("{}", Self::format_json_pretty(&msg));
                                }
                            }
                        }
                        None => {
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                    "Expected ERROR at line {}, but stream ended successfully",
                                    section.start_line
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Ensure we close the request stream
        drop(tx.take());

        // If in update mode, capture any remaining responses
        if let Some(resp) = &mut captured_response {
            if response_stream.is_none()
                && let Some(handle) = call_handle.take()
            {
                match handle.await {
                    Ok(Ok((h, stream))) => {
                        resp.headers = h;
                        response_stream = Some(stream);
                    }
                    Ok(Err(_)) | Err(_) => {
                        response_stream = None;
                    }
                }
            }

            loop {
                let next_item = if let Some(stream) = response_stream.as_mut() {
                    stream.next().await
                } else {
                    None
                };

                let Some(item_res) = next_item else {
                    break;
                };

                match item_res {
                    Ok(crate::grpc::client::StreamItem::Message(msg)) => {
                        resp.messages.push(msg);
                    }
                    Ok(crate::grpc::client::StreamItem::Trailers(t)) => {
                        resp.trailers.extend(t);
                    }
                    Err(status) => {
                        resp.error = Some(status.message().to_string());
                    }
                }
            }
        }

        let grpc_duration = start_time.elapsed().as_millis() as u64;

        if !failure_reasons.is_empty() {
            // Even if failed, we might want to return captured response?
            // Usually snapshot update only happens if user asks for it.
            // If write_mode is true, we should probably ignore failures?
            if effective_write_mode {
                // In write mode, failures (mismatches) are expected because we are updating!
                // But validation errors (like invalid JSON) might still be relevant.
                // Let's assume update mode implies "I want to overwrite whatever happens".
                if let Some(resp) = captured_response {
                    return Ok(TestExecutionResult::pass(Some(grpc_duration)).with_response(resp));
                }
            }

            return Ok(TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                Some(grpc_duration),
            ));
        }

        let mut result = TestExecutionResult::pass(Some(grpc_duration));
        if let Some(resp) = captured_response {
            result = result.with_response(resp);
        }
        Ok(result)
    }

    /// Validates a collected response against the document (for testing purposes)
    pub fn validate_response(
        &self,
        document: &GctfDocument,
        response: &crate::grpc::GrpcResponse,
        _timeout_ms: u64,
    ) -> TestExecutionResult {
        // Delegate to ResponseHandler
        self.response_handler.validate_document(document, response)
    }

    fn metadata_map_to_hashmap(metadata: &tonic::metadata::MetadataMap) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for entry in metadata.iter() {
            match entry {
                KeyAndValueRef::Ascii(key, value) => {
                    if let Ok(v) = value.to_str() {
                        out.insert(key.to_string(), v.to_string());
                    }
                }
                KeyAndValueRef::Binary(key, value) => {
                    out.insert(
                        key.to_string(),
                        String::from_utf8_lossy(value.as_encoded_bytes()).into_owned(),
                    );
                }
            }
        }
        out
    }

    fn substitute_variables(&self, value: &mut Value, variables: &HashMap<String, Value>) {
        match value {
            Value::String(s) => {
                if !s.contains("{{") {
                    return;
                }

                // Check for exact match "{{ var }}" to preserve type
                if s.starts_with("{{") && s.ends_with("}}") {
                    let inner = s[2..s.len() - 2].trim();
                    // check if inner has more {{ }} which implies complex string
                    if !inner.contains("{{")
                        && let Some(val) = variables.get(inner)
                    {
                        *value = val.clone();
                        return;
                    }
                }

                // String interpolation "prefix {{ var }} suffix"
                if let Some(result) = Self::interpolate_variables(s, variables) {
                    *value = Value::String(result);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.substitute_variables(v, variables);
                }
            }
            Value::Object(obj) => {
                for v in obj.values_mut() {
                    self.substitute_variables(v, variables);
                }
            }
            _ => {}
        }
    }

    fn run_assertions(
        &self,
        lines: &[String],
        target_value: &Value,
        failure_reasons: &mut Vec<String>,
        line: usize,
        assertion_context: AssertionContext<'_>,
    ) {
        let mut optimized_lines: Option<Vec<String>> = None;

        for (idx, line) in lines.iter().enumerate() {
            if let Some(rewritten) =
                optimizer::rewrite_assertion_expression_fixed_point_if_changed(line)
            {
                let vec = optimized_lines.get_or_insert_with(|| lines[..idx].to_vec());
                vec.push(rewritten);
            } else if let Some(vec) = optimized_lines.as_mut() {
                vec.push(line.clone());
            }
        }

        let lines_to_evaluate: &[String] = optimized_lines.as_deref().unwrap_or(lines);

        // Use AssertionHandler for assertion evaluation
        let result = self.assertion_handler.evaluate_assertions_for_section(
            lines_to_evaluate,
            target_value,
            assertion_context.headers,
            assertion_context.trailers,
            line,
            assertion_context.timing,
        );

        if !result.passed {
            failure_reasons.extend(result.failure_messages);
        }
    }

    /// Print dry-run preview of test execution
    fn print_dry_run_preview(
        &self,
        document: &GctfDocument,
        address: &str,
        package: &str,
        service: &str,
        method: &str,
    ) {
        println!();
        println!("🔍 Dry-Run Preview: {}", document.file_path);
        println!("═══════════════════════════════════════════════════════════════");
        println!();
        println!("📍 Target:");
        println!("   Address: {}", address);
        let full_service = Self::full_service_name(package, service);
        println!("   Endpoint: {} / {}", full_service, method);
        println!();

        // Display headers first
        let mut has_headers = false;
        for section in &document.sections {
            if section.section_type == SectionType::RequestHeaders {
                if !has_headers {
                    println!();
                    println!("📋 Request Headers:");
                    has_headers = true;
                }
                if let SectionContent::KeyValues(headers) = &section.content {
                    for (key, value) in headers {
                        println!("   {}: {}", key, value);
                    }
                }
            }
        }

        // Group requests and responses to show flow
        let mut has_request = false;
        let mut has_asserts = false;
        let mut has_error = false;

        for section in &document.sections {
            match section.section_type {
                SectionType::Address => {}
                SectionType::Endpoint => {}
                SectionType::RequestHeaders => {}
                SectionType::Options => {}
                SectionType::Tls => {}
                SectionType::Proto => {}
                SectionType::Request => {
                    if !has_request {
                        println!();
                        println!("📤 Request/Response Flow:");
                        has_request = true;
                    }
                    if let SectionContent::Json(json) = &section.content {
                        let json_str = Self::format_json_pretty(json);
                        println!("   ➤ REQUEST:");
                        println!("     {}", json_str.replace('\n', "\n     "));
                    }
                }
                SectionType::Response => {
                    let with_asserts = if section.inline_options.with_asserts {
                        " (with_asserts)"
                    } else {
                        ""
                    };
                    match &section.content {
                        SectionContent::Json(json) => {
                            let json_str = Self::format_json_pretty(json);
                            println!(
                                "   ↤ RESPONSE (Line {}):{}",
                                section.start_line, with_asserts
                            );
                            println!("     {}", json_str.replace('\n', "\n     "));
                        }
                        SectionContent::JsonLines(values) => {
                            println!(
                                "   ↤ RESPONSE (Line {}, {} messages):{}",
                                section.start_line,
                                values.len(),
                                with_asserts
                            );
                            for value in values {
                                let json_str = Self::format_json_pretty(value);
                                println!("     {}", json_str.replace('\n', "\n     "));
                            }
                        }
                        _ => {}
                    }
                }
                SectionType::Asserts => {
                    if !has_asserts {
                        println!();
                        println!("✓ Assertions:");
                        has_asserts = true;
                    }
                    if let SectionContent::Assertions(lines) = &section.content {
                        for line in lines {
                            println!("   . {}", line);
                        }
                    }
                }
                SectionType::Error => {
                    if !has_error {
                        println!();
                        println!("❌ Expected Error:");
                        has_error = true;
                    }
                    if let SectionContent::Json(json) = &section.content {
                        let json_str = Self::format_json_pretty(json);
                        println!("   {}", json_str);
                    }
                }
                SectionType::Extract => {
                    println!();
                    println!("💾 Variables to Extract:");
                    if let SectionContent::Extract(extractions) = &section.content {
                        for (key, query) in extractions {
                            println!("   {} -> {}", key, query);
                        }
                    }
                }
            }
        }

        // Show TLS config if present (including environment defaults)
        let tls_defaults = tls_env_defaults();
        if let Some(tls_config) = document.get_tls_config_with_defaults(&tls_defaults) {
            println!();
            println!("🔒 TLS Configuration:");
            if let Some(ca_cert) = tls_config
                .get("ca_cert")
                .or_else(|| tls_config.get("ca_file"))
            {
                println!("   CA Cert: {}", ca_cert);
            }
            if let Some(client_cert) = tls_config
                .get("client_cert")
                .or_else(|| tls_config.get("cert"))
                .or_else(|| tls_config.get("cert_file"))
            {
                println!("   Client Cert: {}", client_cert);
            }
            if let Some(client_key) = tls_config
                .get("client_key")
                .or_else(|| tls_config.get("key"))
                .or_else(|| tls_config.get("key_file"))
            {
                println!("   Client Key: {}", client_key);
            }
            if tls_config
                .get("insecure")
                .map(|s| parse_bool_flag(s))
                .unwrap_or(false)
            {
                println!("   Insecure Skip Verify: true");
            }
        }

        // Show PROTO config if present
        if let Some(proto_config) = document.get_proto_config() {
            println!();
            println!("📄 Proto Configuration:");
            if proto_config.contains_key("descriptor") {
                println!("   Descriptor: {}", proto_config.get("descriptor").unwrap());
            }
            if proto_config.contains_key("files") {
                println!("   Proto Files: {}", proto_config.get("files").unwrap());
            }
        }

        println!();
        println!("═══════════════════════════════════════════════════════════════");
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_test_runner_new() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        assert!(!runner.dry_run);
        assert_eq!(runner.timeout_seconds, 30);
        assert!(!runner.no_assert);
        assert!(!runner.write_mode);
        assert!(!runner.verbose);
    }

    #[test]
    fn test_test_runner_with_dry_run() {
        let runner = TestRunner::new(true, 30, false, false, false, None);
        assert!(runner.dry_run);
    }

    #[test]
    fn test_test_runner_with_timeout() {
        let runner = TestRunner::new(false, 60, false, false, false, None);
        assert_eq!(runner.timeout_seconds, 60);
    }

    #[test]
    fn test_test_runner_with_no_assert() {
        let runner = TestRunner::new(false, 30, true, false, false, None);
        assert!(runner.no_assert);
    }

    #[test]
    fn test_test_runner_with_write_mode() {
        let runner = TestRunner::new(false, 30, false, true, false, None);
        assert!(runner.write_mode);
    }

    #[test]
    fn test_parse_bool_flag_truthy_values() {
        assert!(parse_bool_flag("true"));
        assert!(parse_bool_flag("1"));
        assert!(parse_bool_flag("YES"));
        assert!(parse_bool_flag("on"));
    }

    #[test]
    fn test_parse_bool_flag_falsy_values() {
        assert!(!parse_bool_flag("false"));
        assert!(!parse_bool_flag("0"));
        assert!(!parse_bool_flag("off"));
        assert!(!parse_bool_flag(""));
    }

    #[test]
    fn test_resolve_tls_path_from_env_uses_cwd() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let cwd = std::env::current_dir().unwrap();
        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = resolve_tls_path("certs/ca.crt", true, document_path);
        assert_eq!(Path::new(&resolved), cwd.join("certs/ca.crt"));
    }

    #[test]
    fn test_resolve_tls_path_from_env_without_fs_capability_returns_relative() {
        if runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = resolve_tls_path("certs/ca.crt", true, document_path);
        assert_eq!(resolved, "certs/ca.crt");
    }

    #[test]
    fn test_resolve_tls_path_from_document_uses_document_dir() {
        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = resolve_tls_path("certs/ca.crt", false, document_path);
        assert_eq!(
            Path::new(&resolved),
            Path::new("tests/fixtures").join("certs").join("ca.crt")
        );
    }

    #[test]
    fn test_tls_env_defaults_uses_grpctestify_prefix() {
        let _guard = ENV_MUTEX.lock().unwrap();

        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE, "/tmp/ca.pem");
            std::env::set_var(
                crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE,
                "/tmp/cert.pem",
            );
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE, "/tmp/key.pem");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME, "localhost");
        }

        let defaults = tls_env_defaults();
        assert_eq!(defaults.get("ca_cert"), Some(&"/tmp/ca.pem".to_string()));
        assert_eq!(
            defaults.get("client_cert"),
            Some(&"/tmp/cert.pem".to_string())
        );
        assert_eq!(
            defaults.get("client_key"),
            Some(&"/tmp/key.pem".to_string())
        );
        assert_eq!(defaults.get("server_name"), Some(&"localhost".to_string()));

        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME);
        }
    }

    #[test]
    fn test_tls_env_defaults_ignores_empty_values() {
        let _guard = ENV_MUTEX.lock().unwrap();

        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE, "");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE, "   ");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE, "");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME, " ");
        }

        let defaults = tls_env_defaults();
        assert!(defaults.is_empty());

        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME);
        }
    }

    #[test]
    fn test_test_runner_with_verbose() {
        let runner = TestRunner::new(false, 30, false, false, true, None);
        assert!(runner.verbose);
    }

    #[test]
    fn test_grpc_code_name_from_numeric() {
        assert_eq!(TestRunner::grpc_code_name_from_numeric(0), Some("OK"));
        assert_eq!(TestRunner::grpc_code_name_from_numeric(5), Some("NotFound"));
        assert_eq!(
            TestRunner::grpc_code_name_from_numeric(13),
            Some("Internal")
        );
        assert_eq!(TestRunner::grpc_code_name_from_numeric(99), None);
    }

    #[test]
    fn test_error_matches_expected_message() {
        let expected = json!({
            "message": "Can't find stub",
            "code": 5
        });
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_code() {
        let expected = json!({
            "code": 5
        });
        let error_text = "status: NotFound, message: \"error\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_wrong_code() {
        let expected = json!({
            "code": 3
        });
        let error_text = "status: NotFound, message: \"error\"";
        assert!(!TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_wrong_message() {
        let expected = json!({
            "message": "Different error"
        });
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(!TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_error_matches_expected_string() {
        let expected = json!("Can't find stub");
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn test_full_service_name() {
        assert_eq!(
            TestRunner::full_service_name("package", "Service"),
            "package.Service"
        );
        assert_eq!(TestRunner::full_service_name("", "Service"), "Service");
    }

    #[test]
    fn test_substitute_variables_exact_match_preserves_type() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("{{ count }}");
        let mut vars = HashMap::new();
        vars.insert("count".to_string(), json!(42));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!(42));
    }

    #[test]
    fn test_substitute_variables_interpolation_single_pass() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("id={{id}}, user={{ user }}, ok={{ok}}");
        let mut vars = HashMap::new();
        vars.insert("id".to_string(), json!(7));
        vars.insert("user".to_string(), json!("alice"));
        vars.insert("ok".to_string(), json!(true));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!("id=7, user=alice, ok=true"));
    }

    #[test]
    fn test_substitute_variables_keeps_unknown_placeholder() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("hello {{known}} and {{unknown}}");
        let mut vars = HashMap::new();
        vars.insert("known".to_string(), json!("world"));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!("hello world and {{unknown}}"));
    }

    #[test]
    fn test_expected_values_for_response_section() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent};

        let section = Section {
            section_type: crate::parser::ast::SectionType::Response,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], json!({"key": "value"}));
    }

    #[test]
    fn test_expected_values_for_json_lines() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent};

        let section = Section {
            section_type: crate::parser::ast::SectionType::Response,
            content: SectionContent::JsonLines(vec![
                json!({"key1": "value1"}),
                json!({"key2": "value2"}),
            ]),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 3,
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_expected_values_for_other_section() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionType};

        // The function returns values for any Json content, not just Response sections
        // This is expected behavior - it extracts Json values regardless of section type
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        // Returns 1 because the content is Json, even though it's a Request section
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], json!({"key": "value"}));
    }

    #[test]
    fn test_metadata_map_to_hashmap_extracts_ascii_values() {
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("code", "EXTERNAL_SERVICE_ERROR_CODE".parse().unwrap());
        metadata.insert("message", "External service error message".parse().unwrap());

        let trailers = TestRunner::metadata_map_to_hashmap(&metadata);
        assert_eq!(
            trailers.get("code"),
            Some(&"EXTERNAL_SERVICE_ERROR_CODE".to_string())
        );
        assert_eq!(
            trailers.get("message"),
            Some(&"External service error message".to_string())
        );
    }

    #[test]
    fn test_assertion_scope_timing_single_message_scope() {
        let mut timing = AssertionScopeTimingState::default();

        let first = timing.finish_scope(0, 12, 1).unwrap();

        assert_eq!(first.elapsed_ms, 12);
        assert_eq!(first.total_elapsed_ms, 12);
        assert_eq!(first.scope_message_count, 1);
        assert_eq!(first.scope_index, 1);
    }

    #[test]
    fn test_assertion_scope_timing_batch_scope_uses_full_section_window() {
        let mut timing = AssertionScopeTimingState::default();

        let batch = timing.finish_scope(0, 27, 2).unwrap();

        assert_eq!(batch.elapsed_ms, 27);
        assert_eq!(batch.total_elapsed_ms, 27);
        assert_eq!(batch.scope_message_count, 2);
        assert_eq!(batch.scope_index, 1);
    }

    #[test]
    fn test_assertion_scope_timing_accumulates_total_duration() {
        let mut timing = AssertionScopeTimingState::default();

        let first = timing.finish_scope(0, 10, 1).unwrap();
        let second = timing.finish_scope(10, 35, 3).unwrap();

        assert_eq!(first.elapsed_ms, 10);
        assert_eq!(first.total_elapsed_ms, 10);
        assert_eq!(second.elapsed_ms, 25);
        assert_eq!(second.total_elapsed_ms, 35);
        assert_eq!(second.scope_message_count, 3);
        assert_eq!(second.scope_index, 2);
    }
}
