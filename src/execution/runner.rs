use super::super::parser::GctfDocument;
use super::runner_helpers;
use super::{AssertionHandler, RequestHandler, RequestSendResult, ResponseHandler};
use crate::assert::{AssertionEngine, JsonComparator, get_json_diff};
use crate::grpc::{GrpcClient, GrpcClientConfig};
use crate::parser::ast::{SectionContent, SectionType};
use crate::plugins::AssertionTiming;
use crate::report::CoverageCollector;
use crate::utils::section_header_line;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

static PLUGIN_REGISTRY: LazyLock<Arc<dyn apif_assert::registry::PluginRegistry>> =
    LazyLock::new(|| Arc::new(crate::execution::plugin_dir::build_plugin_manager()));

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
    pub backend: String,
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
    pub headers: crate::parser::OrderedStringMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    pub index: usize,
    #[serde(default)]
    pub skipped: bool,
    pub content: Value,
    pub content_type: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectationInfo {
    pub index: usize,
    #[serde(default)]
    pub skipped: bool,
    pub expectation_type: String,
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
    #[serde(default)]
    pub skipped: bool,
    pub assertions: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub response_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionInfo {
    pub index: usize,
    #[serde(default)]
    pub skipped: bool,
    pub variables: crate::parser::OrderedStringMap,
    pub line_start: usize,
    pub line_end: usize,
    pub response_index: Option<usize>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcModeInfo {
    Unary,
    ServerStreaming,
    ClientStreaming,
    BidirectionalStreaming,
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
    #[serde(default)]
    pub skipped_sections: usize,
    pub rpc_mode_name: String,
}

struct AssertionContext<'a> {
    headers: &'a HashMap<String, String>,
    trailers: &'a HashMap<String, String>,
    timing: Option<&'a AssertionTiming>,
    variables: &'a HashMap<String, Value>,
    protocol: &'static str,
}

impl ExecutionPlan {
    pub fn from_document(doc: &GctfDocument) -> Self {
        let file_path = doc.file_path.clone();

        let backend = doc
            .first_section(SectionType::Options)
            .and_then(|s| match &s.content {
                SectionContent::KeyValues(kv) => kv.get("protocol").cloned(),
                _ => None,
            })
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| "grpc".to_string());

        let http = doc.transport() == crate::parser::ast::Transport::Http;
        let backend = if http {
            doc.get_address(None)
                .filter(|a| a.trim().starts_with("https://"))
                .map(|_| "https".to_string())
                .unwrap_or_else(|| "http".to_string())
        } else {
            backend
        };

        let connection = if let Some(section) = doc.first_section(SectionType::Address) {
            if let SectionContent::Single(addr) = &section.content {
                ConnectionInfo {
                    address: addr.clone(),
                    source: format!(
                        "ADDRESS section [line {}]",
                        section_header_line(section.start_line)
                    ),
                    backend: backend.clone(),
                }
            } else {
                ConnectionInfo {
                    address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                    source: "Environment variable (implicit)".to_string(),
                    backend: backend.clone(),
                }
            }
        } else {
            ConnectionInfo {
                address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                source: "Environment variable (implicit)".to_string(),
                backend,
            }
        };

        let target = if let Some(section) = doc.first_section(SectionType::Endpoint) {
            if let SectionContent::Single(endpoint) = &section.content {
                let (package, service, method) = if http {
                    (None, None, None)
                } else {
                    doc.parse_endpoint()
                        .map(|(p, s, m)| (Some(p), Some(s), Some(m)))
                        .unwrap_or((None, None, None))
                };
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

        let request_sections = doc.sections_by_type(SectionType::Request);
        let requests: Vec<RequestInfo> = request_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let (content, content_type) = match &section.content {
                    SectionContent::Json(j) => (j.clone(), "json"),
                    SectionContent::JsonLines(vals) => (Value::Array(vals.clone()), "json-lines"),
                    SectionContent::Empty => (Value::Object(serde_json::Map::new()), "empty"),
                    _ => (Value::Null, "unknown"),
                };
                RequestInfo {
                    index: i + 1,
                    skipped: section.get_skip(),
                    content,
                    content_type: content_type.to_string(),
                    line_start: section.start_line,
                    line_end: section.end_line,
                }
            })
            .collect();

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
                        skipped: section.get_skip(),
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
                skipped: section.get_skip(),
                expectation_type: "error".to_string(),
                content,
                message_count: None,
                comparison_options: ComparisonOptions {
                    partial: section.inline_options.partial,
                    redact: section.inline_options.redact.clone(),
                    tolerance: section.inline_options.tolerance,
                    unordered_arrays: section.inline_options.unordered_arrays,
                    with_asserts: section.inline_options.with_asserts,
                },
                line_start: section.start_line,
                line_end: section.end_line,
            }]
        } else {
            vec![]
        };

        let assert_sections = doc.sections_by_type(SectionType::Asserts);
        let assertions: Vec<AssertionInfo> = assert_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let assertions = if let SectionContent::Assertions(lines) = &section.content {
                    lines.clone()
                } else {
                    vec![]
                };
                AssertionInfo {
                    index: i + 1,
                    skipped: section.get_skip(),
                    assertions,
                    line_start: section.start_line,
                    line_end: section.end_line,
                    response_index: None,
                }
            })
            .collect();

        let extract_sections = doc.sections_by_type(SectionType::Extract);
        let extractions: Vec<ExtractionInfo> = extract_sections
            .iter()
            .enumerate()
            .map(|(i, section)| {
                let variables = if let SectionContent::Extract(vars) = &section.content {
                    vars.clone()
                } else {
                    crate::parser::OrderedStringMap::new()
                };
                ExtractionInfo {
                    index: i + 1,
                    skipped: section.get_skip(),
                    variables,
                    line_start: section.start_line,
                    line_end: section.end_line,
                    response_index: None,
                }
            })
            .collect();

        let rpc_mode = infer_rpc_mode(&requests, &expectations, error_section.is_some());

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
            skipped_sections: requests.iter().filter(|r| r.skipped).count()
                + expectations.iter().filter(|e| e.skipped).count()
                + assertions.iter().filter(|a| a.skipped).count()
                + extractions.iter().filter(|e| e.skipped).count(),
            rpc_mode_name: if http {
                doc.parse_http_endpoint()
                    .map(|(m, _)| m)
                    .unwrap_or_else(|| "HTTP".to_string())
            } else {
                rpc_mode_name.to_string()
            },
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

pub struct PreparedDocument {
    address: String,
    package: String,
    service: String,
    method: String,
    full_service: String,
    timeout_seconds: u64,
    compression: apif_grpc_transport::CompressionMode,
    tls_config: Option<apif_grpc_transport::config::TlsConfig>,
    proto_config: Option<apif_grpc_transport::config::ProtoConfig>,
    protocol: crate::grpc::WireProtocol,
    rpc_mode: RpcModeInfo,
}

pub struct PreparedChain(Vec<Option<PreparedDocument>>);

struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);

impl<T> AbortOnDrop<T> {
    fn into_inner(mut self) -> Option<tokio::task::JoinHandle<T>> {
        self.0.take()
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

fn get_actual_rpc_mode(
    client: &crate::grpc::GrpcClient,
    full_service: &str,
    method_name: &str,
) -> RpcModeInfo {
    client
        .descriptor_pool()
        .get_service_by_name(full_service)
        .and_then(|s| s.methods().find(|m| m.name() == method_name))
        .map(|m| {
            if m.is_client_streaming() && m.is_server_streaming() {
                RpcModeInfo::BidirectionalStreaming
            } else if m.is_server_streaming() {
                RpcModeInfo::ServerStreaming
            } else if m.is_client_streaming() {
                RpcModeInfo::ClientStreaming
            } else {
                RpcModeInfo::Unary
            }
        })
        .unwrap_or(RpcModeInfo::Unknown)
}

fn rpc_mode_info(mode: crate::grpc::RpcMode) -> RpcModeInfo {
    match mode {
        crate::grpc::RpcMode::Unary => RpcModeInfo::Unary,
        crate::grpc::RpcMode::ClientStream => RpcModeInfo::ClientStreaming,
        crate::grpc::RpcMode::ServerStream => RpcModeInfo::ServerStreaming,
        crate::grpc::RpcMode::Bidi => RpcModeInfo::BidirectionalStreaming,
    }
}

fn protocol_display(protocol: crate::grpc::WireProtocol) -> &'static str {
    match protocol {
        crate::grpc::WireProtocol::Grpc => "gRPC",
        crate::grpc::WireProtocol::GrpcWeb => "gRPC-Web",
        crate::grpc::WireProtocol::ConnectRpc => "ConnectRPC",
    }
}

fn protocol_str(protocol: crate::grpc::WireProtocol) -> &'static str {
    match protocol {
        crate::grpc::WireProtocol::Grpc => "grpc",
        crate::grpc::WireProtocol::GrpcWeb => "grpc-web",
        crate::grpc::WireProtocol::ConnectRpc => "connectrpc",
    }
}

pub(crate) fn infer_rpc_mode_for_section_types(document: &GctfDocument) -> RpcModeInfo {
    let request_count: usize = document
        .sections_by_type(SectionType::Request)
        .iter()
        .map(|s| match &s.content {
            SectionContent::JsonLines(values) => values.len(),
            _ => 1,
        })
        .sum();
    let response_sections = document.sections_by_type(SectionType::Response);
    let has_json_lines = response_sections
        .iter()
        .any(|s| matches!(&s.content, SectionContent::JsonLines(vals) if vals.len() > 1));
    let has_error = document.first_section(SectionType::Error).is_some();

    if has_error {
        if request_count > 1 {
            RpcModeInfo::ClientStreaming
        } else {
            RpcModeInfo::Unary
        }
    } else if has_json_lines || response_sections.len() > 1 {
        if request_count > 1 {
            RpcModeInfo::BidirectionalStreaming
        } else {
            RpcModeInfo::ServerStreaming
        }
    } else if request_count > 1 {
        RpcModeInfo::ClientStreaming
    } else {
        RpcModeInfo::Unary
    }
}

fn check_rpc_mode_compatibility(inferred: &RpcModeInfo, actual: &RpcModeInfo) -> Option<String> {
    match (inferred, actual) {
        (RpcModeInfo::Unary, RpcModeInfo::Unary) => None,
        (RpcModeInfo::ServerStreaming, RpcModeInfo::ServerStreaming) => None,
        (RpcModeInfo::ClientStreaming, RpcModeInfo::ClientStreaming) => None,
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::BidirectionalStreaming) => None,
        (RpcModeInfo::Unknown, _) => None,
        (_, RpcModeInfo::Unknown) => None,

        (RpcModeInfo::Unary, RpcModeInfo::ServerStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Server Streaming. Client will send ONE request and expect multiple responses.".to_string())
        }
        (RpcModeInfo::Unary, RpcModeInfo::ClientStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Client Streaming. Client will send ONE request but server expects multiple.".to_string())
        }
        (RpcModeInfo::Unary, RpcModeInfo::BidirectionalStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Bidirectional Streaming. Client will send ONE request but server expects stream.".to_string())
        }

        (RpcModeInfo::ServerStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Server Streaming (multiple RESPONSE sections) but proto expects Unary. gCTF may fail.".to_string())
        }
        (RpcModeInfo::ClientStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Client Streaming (multiple REQUEST sections) but proto expects Unary. gCTF may fail.".to_string())
        }
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Bidirectional Streaming but proto expects Unary. gCTF may fail.".to_string())
        }

        (RpcModeInfo::ServerStreaming, RpcModeInfo::ClientStreaming) => {
            Some("gCTF expects Server Streaming but proto declares Client Streaming. Behavior may be incorrect.".to_string())
        }
        (RpcModeInfo::ServerStreaming, RpcModeInfo::BidirectionalStreaming) => {
            Some("gCTF expects Server Streaming but proto declares Bidirectional Streaming. Behavior may be incorrect.".to_string())
        }
        (RpcModeInfo::ClientStreaming, RpcModeInfo::ServerStreaming) => {
            Some("gCTF expects Client Streaming but proto declares Server Streaming. Behavior may be incorrect.".to_string())
        }
        (RpcModeInfo::ClientStreaming, RpcModeInfo::BidirectionalStreaming) => {
            Some("gCTF expects Client Streaming but proto declares Bidirectional Streaming. Behavior may be incorrect.".to_string())
        }
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::ServerStreaming) => {
            Some("gCTF expects Bidirectional Streaming but proto declares Server Streaming. Behavior may be incorrect.".to_string())
        }
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::ClientStreaming) => {
            Some("gCTF expects Bidirectional Streaming but proto declares Client Streaming. Behavior may be incorrect.".to_string())
        }
    }
}

fn infer_rpc_mode(
    requests: &[RequestInfo],
    expectations: &[ExpectationInfo],
    has_error: bool,
) -> RpcMode {
    let req_count: usize = requests
        .iter()
        .map(|r| {
            if r.content_type == "json-lines" {
                r.content.as_array().map_or(1, |v| v.len().max(1))
            } else {
                1
            }
        })
        .sum();
    let resp_count: usize = expectations
        .iter()
        .filter(|e| e.expectation_type == "response")
        .map(|e| e.message_count.unwrap_or(1).max(1))
        .sum();

    if has_error {
        RpcMode::UnaryError
    } else if resp_count > 1 {
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
    } else if req_count == 1 {
        RpcMode::Unary
    } else if req_count == 0 && resp_count > 0 {
        RpcMode::ServerStreaming {
            response_count: resp_count,
        }
    } else {
        RpcMode::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExecutionStatus {
    Pass,
    Fail(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Transport,
    Assertion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestExecutionResult {
    pub status: TestExecutionStatus,
    pub call_duration_ms: Option<u64>,
    pub call_duration_ns: Option<u64>,
    pub captured_response: Option<crate::grpc::GrpcResponse>,
    pub meta: crate::state::TestMeta,
    pub config_summary: apif_state::ConfigSummary,
    pub failure_kind: Option<FailureKind>,
    pub grpc_status: Option<u32>,
    pub assertions: Vec<apif_state::AssertionRecord>,
    pub retried: bool,
    pub dialled_address: Option<String>,
    pub document_durations_ms: Vec<u64>,
    pub extracted: Vec<(String, String)>,
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
    pub fn pass(call_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Pass,
            call_duration_ms,
            call_duration_ns: None,
            captured_response: None,
            meta: crate::state::TestMeta::default(),
            config_summary: apif_state::ConfigSummary::default(),
            failure_kind: None,
            grpc_status: None,
            assertions: Vec::new(),
            retried: false,
            extracted: Vec::new(),
            dialled_address: None,
            document_durations_ms: Vec::new(),
        }
    }

    pub fn dialled(mut self, address: &str) -> Self {
        if !address.trim().is_empty() {
            self.dialled_address = Some(address.to_string());
        }
        self
    }

    pub fn with_call_duration_ns(mut self, ns: u64) -> Self {
        self.call_duration_ns = Some(ns);
        self
    }

    pub fn fail(message: String, call_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Fail(message),
            call_duration_ms,
            call_duration_ns: None,
            captured_response: None,
            meta: crate::state::TestMeta::default(),
            config_summary: apif_state::ConfigSummary::default(),
            failure_kind: Some(FailureKind::Assertion),
            grpc_status: None,
            assertions: Vec::new(),
            retried: false,
            extracted: Vec::new(),
            dialled_address: None,
            document_durations_ms: Vec::new(),
        }
    }

    pub fn with_failure_kind(mut self, kind: FailureKind) -> Self {
        self.failure_kind = Some(kind);
        self
    }

    pub fn with_retried(mut self, retried: bool) -> Self {
        self.retried = retried;
        self
    }

    pub fn with_grpc_status(mut self, code: u32) -> Self {
        self.grpc_status = Some(code);
        self
    }

    pub fn with_response(mut self, response: crate::grpc::GrpcResponse) -> Self {
        self.captured_response = Some(response);
        self
    }

    pub fn with_meta(mut self, meta: crate::state::TestMeta) -> Self {
        self.meta = meta;
        self
    }

    pub fn with_config_summary(mut self, config_summary: apif_state::ConfigSummary) -> Self {
        self.config_summary = config_summary;
        self
    }

    pub fn with_assertions(mut self, assertions: Vec<apif_state::AssertionRecord>) -> Self {
        self.assertions = assertions;
        self
    }
}

#[derive(Default)]
struct ChainAccumulator {
    status: Option<TestExecutionStatus>,
    failure_kind: Option<FailureKind>,
    grpc_status: Option<u32>,
    total_duration_ms: f64,
    total_duration_ns: u64,
    assertions: Vec<apif_state::AssertionRecord>,
    captured_response: Option<crate::grpc::GrpcResponse>,
    retried: bool,
    document_durations_ms: Vec<u64>,
    dialled_address: Option<String>,
    extracted: Vec<(String, String)>,
}

impl ChainAccumulator {
    fn absorb(&mut self, result: TestExecutionResult) -> bool {
        self.absorb_with(result, true)
    }

    fn absorb_group(&mut self, results: Vec<TestExecutionResult>) -> bool {
        let slowest_ms = results
            .iter()
            .filter_map(|r| r.call_duration_ms)
            .max()
            .unwrap_or(0);
        let slowest_ns = results
            .iter()
            .filter_map(|r| r.call_duration_ns)
            .max()
            .unwrap_or(0);
        let mut stop = false;
        for result in results {
            stop |= self.absorb_with(result, false);
        }
        self.total_duration_ms += slowest_ms as f64;
        self.total_duration_ns += slowest_ns;
        stop
    }

    fn absorb_with(&mut self, mut result: TestExecutionResult, charge: bool) -> bool {
        if charge && let Some(dur) = result.call_duration_ms {
            self.total_duration_ms += dur as f64;
        }
        if charge {
            self.total_duration_ns += result.call_duration_ns.unwrap_or(0);
        }
        self.document_durations_ms
            .push(result.call_duration_ms.unwrap_or(0));
        self.grpc_status = result.grpc_status;
        self.retried |= result.retried;
        if result.dialled_address.is_some() {
            self.dialled_address = result.dialled_address.clone();
        }
        self.assertions.append(&mut result.assertions);
        if result.captured_response.is_some() {
            self.captured_response = result.captured_response.take();
        }

        if matches!(result.status, TestExecutionStatus::Fail(_)) {
            if self.status.is_none() {
                self.failure_kind = result.failure_kind;
                self.status = Some(result.status);
            }
            return true;
        }
        false
    }

    fn into_result(self) -> TestExecutionResult {
        TestExecutionResult {
            status: self.status.unwrap_or(TestExecutionStatus::Pass),
            call_duration_ms: Some(self.total_duration_ms as u64),
            call_duration_ns: Some(self.total_duration_ns),
            captured_response: self.captured_response,
            meta: crate::state::TestMeta::default(),
            config_summary: apif_state::ConfigSummary::default(),
            failure_kind: self.failure_kind,
            grpc_status: self.grpc_status,
            assertions: self.assertions,
            retried: self.retried,
            dialled_address: self.dialled_address,
            document_durations_ms: self.document_durations_ms,
            extracted: self.extracted,
        }
    }
}

pub struct TestRunner {
    dry_run: bool,
    timeout_seconds: u64,
    no_assert: bool,
    write_mode: bool,
    verbose: bool,
    protocol_override: Option<crate::grpc::WireProtocol>,
    connection_id: u64,
    address_override: Option<String>,
    env_address: Option<String>,
    capture_exchange: bool,
    assertion_engine: AssertionEngine,
    coverage_collector: Option<Arc<CoverageCollector>>,
    request_handler: RequestHandler,
    response_handler: ResponseHandler,
    assertion_handler: AssertionHandler,
}

fn response_record(
    section: &crate::parser::ast::Section,
    diffs: &[crate::assert::AssertionResult],
    expected: &Value,
    actual: &Value,
) -> apif_state::AssertionRecord {
    let passed = diffs.is_empty();
    let message = diffs.iter().find_map(|d| match d {
        crate::assert::AssertionResult::Fail { message, .. } => Some(message.clone()),
        crate::assert::AssertionResult::Error(m) => Some(m.clone()),
        crate::assert::AssertionResult::Pass => None,
    });
    apif_state::AssertionRecord {
        line: section_header_line(section.start_line),
        expression: section.format_header(),
        passed,
        elapsed_ms: 0,
        message,
        endpoint: None,
        expected: (!passed).then(|| runner_helpers::format_json_pretty(expected)),
        actual: (!passed).then(|| runner_helpers::format_json_pretty(actual)),
        hint: None,
    }
}

pub(crate) fn chain_addresses(document: &GctfDocument) -> Vec<Option<String>> {
    let mut grpc: Option<String> = None;
    let mut http: Option<String> = None;
    document
        .iter_chain()
        .map(|doc| {
            let inherited = match doc.transport() {
                crate::parser::ast::Transport::Http => &mut http,
                crate::parser::ast::Transport::Grpc => &mut grpc,
            };
            if let Some(own) = doc.get_address(None).filter(|a| !a.trim().is_empty()) {
                *inherited = Some(own);
            }
            inherited.clone()
        })
        .collect()
}

impl TestRunner {
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

    fn has_required_followup_asserts(
        section: &crate::parser::ast::Section,
        sections: &[crate::parser::ast::Section],
        index: usize,
        effective_no_assert: bool,
        failure_reasons: &mut Vec<String>,
    ) -> bool {
        if !section.inline_options.with_asserts {
            return false;
        }

        if sections
            .get(index + 1)
            .is_some_and(|next| next.section_type == SectionType::Asserts)
        {
            return true;
        }

        if !effective_no_assert {
            failure_reasons.push(format!(
                "{} at line {} has 'with_asserts' but is not followed by ASSERTS",
                section.section_type.as_str(),
                section_header_line(section.start_line)
            ));
        }

        false
    }

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
            protocol_override: None,
            connection_id: 0,
            address_override: None,
            env_address: None,
            capture_exchange: false,
            assertion_engine: AssertionEngine::with_registry(PLUGIN_REGISTRY.clone()),
            coverage_collector: coverage_collector.clone(),
            request_handler: RequestHandler::new(no_assert, verbose, coverage_collector.clone()),
            response_handler: ResponseHandler::new(no_assert),
            assertion_handler: AssertionHandler::new(verbose),
        }
    }

    pub fn with_protocol(mut self, protocol: crate::grpc::WireProtocol) -> Self {
        self.protocol_override = Some(protocol);
        self
    }

    pub fn with_address_override(mut self, address: String) -> Self {
        self.address_override = Some(address);
        self
    }

    pub fn with_env_address(mut self, address: String) -> Self {
        self.env_address = Some(address);
        self
    }

    pub fn with_capture_exchange(mut self, capture: bool) -> Self {
        self.capture_exchange = capture;
        self
    }

    pub fn with_connection_id(mut self, connection_id: u64) -> Self {
        self.connection_id = connection_id;
        self
    }

    pub fn is_write_mode(&self) -> bool {
        self.write_mode
    }

    pub async fn run_test(&self, document: &GctfDocument) -> Result<TestExecutionResult> {
        self.run_test_with_variables(document, HashMap::new()).await
    }

    fn resolve_protocol(&self, document: &GctfDocument) -> crate::grpc::WireProtocol {
        self.protocol_override.unwrap_or_else(|| {
            document
                .get_options()
                .and_then(|o| {
                    o.get("protocol").map(|s| {
                        s.parse::<crate::grpc::WireProtocol>()
                            .unwrap_or(crate::grpc::WireProtocol::Grpc)
                    })
                })
                .unwrap_or(crate::grpc::WireProtocol::Grpc)
        })
    }

    pub fn prepare(&self, document: &GctfDocument) -> PreparedChain {
        let mut chain_address: Option<String> = None;
        PreparedChain(
            document
                .iter_chain()
                .map(|doc| {
                    if let Some(own) = doc.get_address(None).filter(|a| !a.trim().is_empty()) {
                        chain_address = Some(own);
                    }
                    let options = doc.get_options().unwrap_or_default();
                    let (package, service, method) = doc.parse_endpoint()?;
                    let timeout_seconds = match options.get("timeout") {
                        Some(v) => match v.trim().parse::<u64>() {
                            Ok(v) if v > 0 => v,
                            _ => return None,
                        },
                        None => self.timeout_seconds,
                    };
                    let compression = runner_helpers::resolve_compression(
                        doc,
                        &options,
                        crate::config::compression_from_env(),
                    )
                    .ok()?;
                    let document_path = Path::new(&doc.file_path);
                    Some(PreparedDocument {
                        address: match &self.address_override {
                            Some(a) => a.clone(),
                            None => runner_helpers::effective_address_with(
                                doc,
                                self.protocol_override,
                                chain_address.as_deref().or(self.env_address.as_deref()),
                            ),
                        },
                        full_service: runner_helpers::full_service_name(&package, &service),
                        package,
                        service,
                        method,
                        timeout_seconds,
                        compression,
                        tls_config: runner_helpers::build_tls_config(doc, document_path),
                        proto_config: runner_helpers::build_proto_config(doc, document_path),
                        protocol: self.resolve_protocol(doc),
                        rpc_mode: infer_rpc_mode_for_section_types(doc),
                    })
                })
                .collect(),
        )
    }

    pub async fn run_test_prepared(
        &self,
        document: &GctfDocument,
        initial_variables: HashMap<String, Value>,
        prepared: &PreparedChain,
    ) -> Result<TestExecutionResult> {
        let mut variables = initial_variables;
        let mut acc = ChainAccumulator::default();

        for (doc, prep) in document.iter_chain().zip(prepared.0.iter()) {
            let result = self
                .run_one(doc, &mut variables, prep.as_ref(), None)
                .await?;
            if acc.absorb(result) {
                break;
            }
        }

        Ok(acc.into_result())
    }

    fn extracted_by(
        document: &GctfDocument,
        variables: &HashMap<String, Value>,
    ) -> Vec<(String, String)> {
        const MAX_VALUE_BYTES: usize = 4 * 1024;

        let mut out = Vec::new();
        for doc in document.iter_chain() {
            for section in doc.sections_by_type(SectionType::Extract) {
                let SectionContent::Extract(bindings) = &section.content else {
                    continue;
                };
                for (name, _) in bindings.iter() {
                    if out.iter().any(|(seen, _): &(String, String)| seen == name) {
                        continue;
                    }
                    let Some(value) = variables.get(name) else {
                        continue;
                    };
                    let rendered = match value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    if rendered.len() > MAX_VALUE_BYTES {
                        continue;
                    }
                    out.push((name.clone(), rendered));
                }
            }
        }
        out
    }

    pub async fn run_test_with_variables(
        &self,
        document: &GctfDocument,
        initial_variables: HashMap<String, Value>,
    ) -> Result<TestExecutionResult> {
        Ok(self.run_chain(document, initial_variables).await?.0)
    }

    pub async fn run_test_capturing_vars(
        &self,
        document: &GctfDocument,
    ) -> Result<(TestExecutionResult, HashMap<String, Value>)> {
        self.run_chain(document, HashMap::new()).await
    }

    pub async fn run_chain(
        &self,
        document: &GctfDocument,
        initial_variables: HashMap<String, Value>,
    ) -> Result<(TestExecutionResult, HashMap<String, Value>)> {
        let mut variables = initial_variables;
        let mut acc = ChainAccumulator::default();
        let addresses = chain_addresses(document);
        let steps: Vec<&GctfDocument> = document.iter_chain().collect();

        if let Some(nowhere) = self.step_with_nowhere_to_go(&steps, &addresses) {
            let mut refused = TestExecutionResult::pass(None);
            refused.status = TestExecutionStatus::Fail(nowhere);
            return Ok((refused, variables));
        }

        let mut step = 0;
        while step < steps.len() {
            if !steps[step].runs_in_parallel() {
                let chain_address = addresses.get(step).cloned().flatten();
                let result = self
                    .run_step(steps[step], &mut variables, chain_address.as_deref())
                    .await?;
                if acc.absorb(result) {
                    break;
                }
                step += 1;
                continue;
            }

            let mut end = step;
            while end < steps.len() && steps[end].runs_in_parallel() {
                end += 1;
            }
            let bound = variables.clone();
            let group = futures::future::join_all(steps[step..end].iter().enumerate().map(
                |(offset, doc)| {
                    let mut own = bound.clone();
                    let chain_address = addresses.get(step + offset).cloned().flatten();
                    async move {
                        let result = self
                            .run_step(doc, &mut own, chain_address.as_deref())
                            .await?;
                        Ok::<_, anyhow::Error>((own, result))
                    }
                },
            ))
            .await;

            let mut results = Vec::with_capacity(group.len());
            for outcome in group {
                let (own, result) = outcome?;
                for (name, value) in own {
                    if bound.get(&name) != Some(&value) {
                        variables.insert(name, value);
                    }
                }
                results.push(result);
            }
            let stop = acc.absorb_group(results);
            step = end;
            if stop {
                break;
            }
        }

        acc.extracted = Self::extracted_by(document, &variables);
        Ok((acc.into_result(), variables))
    }

    fn step_with_nowhere_to_go(
        &self,
        steps: &[&GctfDocument],
        addresses: &[Option<String>],
    ) -> Option<String> {
        if self.address_override.is_some() || self.env_address.is_some() {
            return None;
        }
        steps.iter().enumerate().find_map(|(index, doc)| {
            if doc.transport() != crate::parser::ast::Transport::Http {
                return None;
            }
            if addresses.get(index).cloned().flatten().is_some() {
                return None;
            }
            let path = doc
                .parse_http_endpoint()
                .map(|(_, path)| path)
                .unwrap_or_else(|| "this step".to_string());
            if path.starts_with("http://") || path.starts_with("https://") {
                return None;
            }
            Some(format!(
                "no address for {path}: an HTTP call needs a target with a scheme — this file's ADDRESS, or the environment's written as `http://host:port` — and nothing was dialled"
            ))
        })
    }

    async fn run_step(
        &self,
        doc: &GctfDocument,
        variables: &mut HashMap<String, Value>,
        chain_address: Option<&str>,
    ) -> Result<TestExecutionResult> {
        if doc.transport() == crate::parser::ast::Transport::Http {
            Ok(self.run_http(doc, variables, chain_address).await)
        } else {
            self.run_one(doc, variables, None, chain_address).await
        }
    }

    async fn run_one(
        &self,
        document: &GctfDocument,
        variables: &mut HashMap<String, Value>,
        prepared: Option<&PreparedDocument>,
        chain_address: Option<&str>,
    ) -> Result<TestExecutionResult> {
        let effective_dry_run = self.dry_run;
        let effective_no_assert = self.no_assert;
        let effective_write_mode = self.write_mode;

        let options = match prepared {
            Some(_) => Default::default(),
            None => document.get_options().unwrap_or_default(),
        };
        let effective_timeout_seconds = match prepared.map(|p| p.timeout_seconds) {
            Some(t) => t,
            None => match options.get("timeout") {
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
            },
        };

        let compression = match prepared.map(|p| p.compression) {
            Some(c) => c,
            None => match runner_helpers::resolve_compression(
                document,
                &options,
                crate::config::compression_from_env(),
            ) {
                Ok(c) => c,
                Err(e) => return Ok(TestExecutionResult::fail(e, None)),
            },
        };

        if effective_write_mode {
            let file_path = Path::new(&document.file_path);
            if !file_path.exists() {
                return Ok(TestExecutionResult::fail(
                    format!("Update mode: file '{}' does not exist", document.file_path),
                    None,
                ));
            }

            use std::fs::OpenOptions;
            if OpenOptions::new().write(true).open(file_path).is_err() {
                return Ok(TestExecutionResult::fail(
                    format!("Update mode: file '{}' is not writable", document.file_path),
                    None,
                ));
            }
        }

        let address = match prepared {
            Some(p) => p.address.clone(),
            None => match &self.address_override {
                Some(a) => a.clone(),
                None => runner_helpers::effective_address_with(
                    document,
                    self.protocol_override,
                    chain_address.or(self.env_address.as_deref()),
                ),
            },
        };
        let address = runner_helpers::interpolate_variables(&address, variables).unwrap_or(address);

        let endpoint_parts =
            prepared.map(|p| (p.package.clone(), p.service.clone(), p.method.clone()));
        let (package, service, method) = match endpoint_parts.or_else(|| document.parse_endpoint())
        {
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
            self.print_dry_run_preview(document, &address, &package, &service, &method);
            return Ok(TestExecutionResult::pass(None));
        }

        let document_path = Path::new(&document.file_path);

        let tls_config = match prepared {
            Some(p) => p.tls_config.clone(),
            None => runner_helpers::build_tls_config(document, document_path),
        };

        let proto_config = match prepared {
            Some(p) => p.proto_config.clone(),
            None => runner_helpers::build_proto_config(document, document_path),
        };

        let full_service = match prepared {
            Some(p) => p.full_service.clone(),
            None => runner_helpers::full_service_name(&package, &service),
        };

        let request_metadata = match document.get_request_headers() {
            Some(headers) => {
                let mut substituted = HashMap::with_capacity(headers.len());
                let mut unresolved = Vec::new();
                for (key, val) in headers {
                    let new_val =
                        runner_helpers::interpolate_variables(&val, variables).unwrap_or(val);
                    runner_helpers::find_unresolved_placeholders(
                        &new_val,
                        variables,
                        &mut unresolved,
                    );
                    substituted.insert(key, new_val);
                }
                if !unresolved.is_empty() {
                    tracing::debug!(
                        "unresolved placeholder(s) in REQUEST_HEADERS: {:?} (known variables: {:?})",
                        unresolved,
                        variables.keys().collect::<Vec<_>>()
                    );
                    return Ok(TestExecutionResult::fail(
                        format!(
                            "Unresolved variable placeholder(s) in REQUEST_HEADERS: {}",
                            runner_helpers::format_unresolved_placeholders(&unresolved)
                        ),
                        None,
                    )
                    .with_failure_kind(FailureKind::Assertion));
                }
                Some(substituted)
            }
            None => None,
        };

        let client_config = GrpcClientConfig {
            address,
            timeout_seconds: effective_timeout_seconds,
            tls_config,
            proto_config,
            metadata: request_metadata,
            target_service: Some(full_service.clone()),
            compression,
            connection_id: self.connection_id,
            protocol: match prepared {
                Some(p) => p.protocol,
                None => self.resolve_protocol(document),
            },
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let client_protocol = client_config.protocol;
        let client_address = client_config.address.clone();
        tracing::debug!(
            "dialing {} ({:?}, service={}, timeout={}s, tls={}, proto_source={})",
            client_address,
            client_protocol,
            full_service,
            client_config.timeout_seconds,
            client_config.tls_config.is_some(),
            if client_config.proto_config.is_some() {
                "explicit"
            } else {
                "reflection"
            }
        );
        let client = GrpcClient::new(client_config).await?;

        let (input_message_type, output_message_type) = if self.coverage_collector.is_some() {
            client
                .descriptor_pool()
                .get_service_by_name(&full_service)
                .and_then(|s| s.methods().find(|m| m.name() == method))
                .map_or((None, None), |m| {
                    (
                        Some(m.input().full_name().to_string()),
                        Some(m.output().full_name().to_string()),
                    )
                })
        } else {
            (None, None)
        };

        let inferred_rpc_mode = match prepared {
            Some(p) => p.rpc_mode.clone(),
            None => infer_rpc_mode_for_section_types(document),
        };
        let descriptor_rpc_mode = if client_protocol == crate::grpc::WireProtocol::Grpc
            && !tracing::enabled!(tracing::Level::DEBUG)
        {
            RpcModeInfo::Unknown
        } else {
            get_actual_rpc_mode(&client, &full_service, &method)
        };
        if tracing::enabled!(tracing::Level::DEBUG)
            && let Some(warning) =
                check_rpc_mode_compatibility(&inferred_rpc_mode, &descriptor_rpc_mode)
        {
            tracing::debug!("{} (service={}, method={})", warning, full_service, method);
        }

        let wire_rpc_mode = match (client_protocol, &descriptor_rpc_mode) {
            (crate::grpc::WireProtocol::Grpc, _) => inferred_rpc_mode.clone(),
            (_, RpcModeInfo::Unknown) => client
                .reflect_rpc_mode(&full_service, &method)
                .await
                .map_or_else(|| inferred_rpc_mode.clone(), rpc_mode_info),
            (_, actual) => actual.clone(),
        };

        let (tx, rx) = mpsc::channel::<Value>(runner_helpers::REQUEST_CHANNEL_BUFFER);
        let request_stream = ReceiverStream::new(rx);
        let mut tx = Some(tx);

        if let Some(collector) = &self.coverage_collector {
            collector.register_pool(client.descriptor_pool());
            collector.record_call(&full_service, &method);
        }

        let start_time = std::time::Instant::now();

        let rpc_mode: Option<crate::grpc::RpcMode> = match wire_rpc_mode {
            RpcModeInfo::Unary => Some(crate::grpc::RpcMode::Unary),
            RpcModeInfo::ServerStreaming => Some(crate::grpc::RpcMode::ServerStream),
            RpcModeInfo::ClientStreaming => Some(crate::grpc::RpcMode::ClientStream),
            RpcModeInfo::BidirectionalStreaming => Some(crate::grpc::RpcMode::Bidi),
            RpcModeInfo::Unknown => None,
        };

        let is_http_bidi = client_protocol != crate::grpc::WireProtocol::Grpc
            && rpc_mode == Some(crate::grpc::RpcMode::Bidi);
        let mut deferred_bidi_expectations: Vec<Value> = Vec::new();

        let full_service_clone = full_service.clone();
        let method_clone = method.clone();
        let mut client_for_call = client;
        let mut call_handle = Some(AbortOnDrop(Some(tokio::spawn(Box::pin(async move {
            client_for_call
                .call_stream(&full_service_clone, &method_clone, request_stream, rpc_mode)
                .await
        })))));

        let mut response_stream = None;

        let mut rpc_end: Option<std::time::Instant> = None;
        let mut last_message: Option<Value> = None;
        let mut last_error_message: Option<String> = None;
        let mut last_error_json: Option<Value> = None;
        let mut last_error_timing: Option<AssertionTiming> = None;
        let mut captured_headers: HashMap<String, String> = HashMap::new();
        let mut captured_trailers: HashMap<String, String> = HashMap::new();
        let mut failure_reasons: Vec<String> = Vec::new();
        let mut assertion_records: Vec<apif_state::AssertionRecord> = Vec::new();
        let mut assertion_timing = AssertionScopeTimingState::default();
        let mut transport_failure = false;
        let mut retry_occurred = false;
        let mut grpc_status: Option<u32> = None;

        let sections = &document.sections;

        let last_request_idx = sections
            .iter()
            .rposition(|s| s.section_type == SectionType::Request);

        let has_request_sections = sections
            .iter()
            .any(|s| s.section_type == SectionType::Request && !s.get_skip());

        if !has_request_sections && let Some(tx_ref) = tx.as_mut() {
            if let Err(e) = tx_ref.send(Value::Object(serde_json::Map::new())).await {
                failure_reasons.push(format!("Failed to send implicit empty request: {}", e));
                transport_failure = true;
            }
            drop(tx.take());
        }

        let mut skip_next_section = false;

        let mut captured_response = if effective_write_mode || self.capture_exchange {
            Some(crate::grpc::GrpcResponse::new())
        } else {
            None
        };

        macro_rules! ensure_stream_ready {
            () => {
                if response_stream.is_none()
                    && let Some(handle) = call_handle.take().and_then(AbortOnDrop::into_inner)
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
                            transport_failure = true;
                            break;
                        }
                        Err(e) => {
                            failure_reasons
                                .push(format!("Failed to join gRPC stream startup task: {}", e));
                            transport_failure = true;
                            break;
                        }
                    }
                }
            };
        }

        let mut inherited_attrs: Vec<crate::parser::ast::GctfAttribute> = Vec::new();

        for (i, section) in sections.iter().enumerate() {
            let resolved_attrs = crate::parser::content_parser::resolve_attributes(
                &section.attributes,
                &inherited_attrs,
            );
            inherited_attrs =
                crate::parser::content_parser::inheritable_attributes(&resolved_attrs);

            let get_attr = |name: &str| -> Option<&crate::parser::ast::GctfAttribute> {
                resolved_attrs.iter().find(|a| a.name == name)
            };
            let get_timeout = || -> Option<u64> {
                get_attr("timeout")
                    .and_then(|a| a.parse_u64())
                    .filter(|&v| v > 0)
            };
            let get_retry = || -> Option<u32> { get_attr("retry").and_then(|a| a.parse_u32()) };
            let get_repeat = || -> Option<u32> {
                get_attr("repeat")
                    .and_then(|a| a.parse_u32())
                    .filter(|&v| v >= 1)
            };
            let get_skip = || -> bool {
                get_attr("skip")
                    .and_then(|a| a.parse_bool())
                    .unwrap_or(false)
            };

            if skip_next_section {
                skip_next_section = false;
                continue;
            }

            if get_skip() {
                continue;
            }

            let repeat_count = get_repeat().unwrap_or(1);
            'repeat_iters: for repeat_iter in 0..repeat_count {
                if repeat_count > 1 {
                    eprintln!(
                        "   [repeat] section at line {} — iteration {}/{}",
                        section_header_line(section.start_line),
                        repeat_iter + 1,
                        repeat_count
                    );
                }

                match section.section_type {
                    SectionType::Request => {
                        let request_values: Vec<Value> = match &section.content {
                            SectionContent::Json(req_json) => vec![req_json.clone()],
                            SectionContent::JsonLines(values) => values.clone(),
                            SectionContent::Empty => vec![Value::Object(serde_json::Map::new())],
                            _ => continue,
                        };

                        for mut request_value in request_values {
                            let may_substitute = !document.metadata.placeholder_free;
                            if may_substitute && !matches!(section.content, SectionContent::Empty) {
                                self.substitute_variables(&mut request_value, variables);
                                let mut unresolved = Vec::new();
                                runner_helpers::collect_unresolved_placeholders(
                                    &request_value,
                                    variables,
                                    &mut unresolved,
                                );
                                if !unresolved.is_empty() {
                                    return Ok(TestExecutionResult::fail(
                                        format!(
                                            "Unresolved variable placeholder(s) in REQUEST at line {}: {}",
                                            section_header_line(section.start_line),
                                            runner_helpers::format_unresolved_placeholders(
                                                &unresolved,
                                            )
                                        ),
                                        Some(start_time.elapsed().as_millis() as u64),
                                    )
                                    .with_failure_kind(FailureKind::Assertion));
                                }
                            }

                            if let (Some(collector), Some(msg_type)) =
                                (&self.coverage_collector, &input_message_type)
                            {
                                collector.record_fields_from_json(msg_type, &request_value);
                            }

                            let Some(tx_ref) = tx.as_mut() else {
                                failure_reasons.push(format!(
                                    "Failed to send request at line {}: request stream already closed",
                                    section_header_line(section.start_line)
                                ));
                                break 'repeat_iters;
                            };

                            let section_timeout = get_timeout();
                            let effective_timeout =
                                section_timeout.unwrap_or(effective_timeout_seconds);
                            let max_retries = get_retry().unwrap_or(0);
                            let mut attempt = 0;

                            let send_with_timeout = |payload: Value| async {
                                if effective_timeout > 0 {
                                    let send_fut = self.request_handler.send_request(
                                        tx_ref,
                                        payload,
                                        section_header_line(section.start_line),
                                        None,
                                    );
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(effective_timeout),
                                        send_fut,
                                    )
                                    .await
                                    {
                                        Ok(r) => r,
                                        Err(_) => RequestSendResult {
                                            success: false,
                                            error_message: Some(format!(
                                                "Request timed out after {}s (section timeout)",
                                                effective_timeout
                                            )),
                                        },
                                    }
                                } else {
                                    self.request_handler
                                        .send_request(
                                            tx_ref,
                                            payload,
                                            section_header_line(section.start_line),
                                            None,
                                        )
                                        .await
                                }
                            };

                            let result = loop {
                                if attempt > 0 {
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        100 * attempt as u64,
                                    ))
                                    .await;
                                }
                                let payload = if attempt < max_retries {
                                    request_value.clone()
                                } else {
                                    std::mem::take(&mut request_value)
                                };
                                let r = send_with_timeout(payload).await;
                                if r.success || attempt >= max_retries {
                                    if r.success && attempt > 0 {
                                        retry_occurred = true;
                                    }
                                    break r;
                                }
                                tracing::debug!(
                                    "retry {}/{} at line {}: {:?}",
                                    attempt + 1,
                                    max_retries,
                                    section_header_line(section.start_line),
                                    r.error_message
                                );
                                attempt += 1;
                            };
                            if !result.success
                                && let Some(error) = result.error_message
                            {
                                failure_reasons.push(error);
                                transport_failure = true;
                            }
                        }
                    }
                    SectionType::Response => {
                        let scope_start_ms = assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                        let mut scope_end_ms = scope_start_ms;
                        let mut scope_message_count = 0usize;

                        if i >= last_request_idx.unwrap_or(usize::MAX) {
                            drop(tx.take());
                        }

                        if is_http_bidi && i < last_request_idx.unwrap_or(usize::MAX) {
                            let expected_values =
                                Self::expected_values_for_response_section(section);
                            for exp in expected_values {
                                deferred_bidi_expectations.push(exp);
                            }
                            continue;
                        }

                        ensure_stream_ready!();

                        let mut received_messages_for_section: Vec<Value> = Vec::new();
                        let section_expected = Self::expected_values_for_response_section(section);
                        let expected_values: Vec<Value> = deferred_bidi_expectations
                            .drain(..)
                            .chain(section_expected)
                            .collect();

                        let Some(stream) = response_stream.as_mut() else {
                            failure_reasons.push(format!(
                                "No response stream available for RESPONSE section at line {}",
                                section_header_line(section.start_line)
                            ));
                            transport_failure = true;
                            break;
                        };

                        let read_timeout_secs = get_timeout().unwrap_or(effective_timeout_seconds);

                        if expected_values.is_empty()
                            && matches!(section.content, SectionContent::Empty)
                            && !effective_no_assert
                        {
                            let next_item = if read_timeout_secs > 0 {
                                tokio::time::timeout(
                                    std::time::Duration::from_secs(read_timeout_secs),
                                    stream.next(),
                                )
                                .await
                                .unwrap_or(None)
                            } else {
                                stream.next().await
                            };
                            if let Some(Ok(crate::grpc::client::StreamItem::Message(msg))) =
                                next_item
                            {
                                rpc_end = Some(std::time::Instant::now());
                                failure_reasons.push(format!(
                                    "RESPONSE section at line {} expects no messages, but the stream produced one: {}",
                                    section_header_line(section.start_line),
                                    msg
                                ));
                            }
                        }

                        let mut stream_read_timed_out = false;
                        for expected_template in expected_values {
                            let next_item = if read_timeout_secs > 0 {
                                match tokio::time::timeout(
                                    std::time::Duration::from_secs(read_timeout_secs),
                                    stream.next(),
                                )
                                .await
                                {
                                    Ok(item) => item,
                                    Err(_) => {
                                        failure_reasons.push(format!(
                                            "Timed out after {}s waiting for stream message for RESPONSE section at line {}",
                                            read_timeout_secs, section_header_line(section.start_line)
                                        ));
                                        transport_failure = true;
                                        stream_read_timed_out = true;
                                        break;
                                    }
                                }
                            } else {
                                stream.next().await
                            };
                            match next_item {
                                Some(Ok(item)) => match item {
                                    crate::grpc::client::StreamItem::Message(msg) => {
                                        rpc_end = Some(std::time::Instant::now());
                                        let now_elapsed_ms =
                                            start_time.elapsed().as_millis() as u64;

                                        last_message = Some(msg.clone());
                                        if section.inline_options.with_asserts {
                                            received_messages_for_section.push(msg.clone());
                                        }
                                        scope_end_ms = now_elapsed_ms;
                                        scope_message_count += 1;
                                        assertion_timing.last_message_elapsed_ms =
                                            Some(now_elapsed_ms);
                                        if let Some(resp) = &mut captured_response {
                                            resp.messages.push(msg.clone());
                                        }

                                        Self::log_response_message(
                                            &msg,
                                            self.verbose,
                                            protocol_display(client_protocol),
                                            &client_address,
                                        );

                                        if !effective_no_assert {
                                            let mut expected = expected_template.clone();
                                            self.substitute_variables(&mut expected, variables);

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

                                            assertion_records.push(response_record(
                                                section, &diffs, &expected, &msg,
                                            ));

                                            if !diffs.is_empty() {
                                                self.append_response_diffs(
                                                    diffs,
                                                    section_header_line(section.start_line),
                                                    &expected,
                                                    &msg,
                                                    &mut failure_reasons,
                                                );
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
                                                section_header_line(section.start_line)
                                            ));
                                        }
                                        break;
                                    }
                                },
                                Some(Err(status)) => {
                                    rpc_end = Some(std::time::Instant::now());
                                    grpc_status = Some(status.code());
                                    let scope_start_ms =
                                        assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                                    let scope_end_ms = start_time.elapsed().as_millis() as u64;
                                    assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                                    last_error_timing = assertion_timing.finish_scope(
                                        scope_start_ms,
                                        scope_end_ms,
                                        1,
                                    );
                                    last_error_message = Some(status.message().to_string());

                                    if let Some(resp) = &mut captured_response {
                                        resp.error = Some(status.message().to_string());
                                    }
                                    if !effective_no_assert {
                                        failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but received Error: {}",
                                        section_header_line(section.start_line),
                                        status.message()
                                    ));
                                    } else if self.verbose {
                                        println!("--- RESPONSE (Error) ---");
                                        println!("{}", status.message());
                                    }
                                    break;
                                }
                                None => {
                                    if !effective_no_assert {
                                        failure_reasons.push(format!(
                                        "Expected message for RESPONSE section at line {}, but stream ended",
                                        section_header_line(section.start_line)
                                    ));
                                    }
                                    break;
                                }
                            }
                        }

                        if stream_read_timed_out {
                            response_stream = None;
                        }

                        if !stream_read_timed_out && let Some(stream) = response_stream.as_mut() {
                            let attaches_next = section.inline_options.with_asserts
                                && matches!(
                                    sections.get(i + 1).map(|s| s.section_type),
                                    Some(SectionType::Asserts)
                                );
                            let further_start = if attaches_next { i + 2 } else { i + 1 };
                            let is_last_reader = sections
                                .get(further_start..)
                                .map(|rest| {
                                    !rest.iter().any(|s| {
                                        matches!(
                                            s.section_type,
                                            SectionType::Response
                                                | SectionType::Asserts
                                                | SectionType::Error
                                        )
                                    })
                                })
                                .unwrap_or(true);

                            if is_last_reader {
                                loop {
                                    let item = if read_timeout_secs > 0 {
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(read_timeout_secs),
                                            stream.next(),
                                        )
                                        .await
                                        {
                                            Ok(it) => it,
                                            Err(_) => break,
                                        }
                                    } else {
                                        stream.next().await
                                    };
                                    match item {
                                        Some(Ok(crate::grpc::client::StreamItem::Trailers(t))) => {
                                            if let Some(resp) = &mut captured_response {
                                                resp.trailers.extend(
                                                    t.iter().map(|(k, v)| (k.clone(), v.clone())),
                                                );
                                            }
                                            captured_trailers.extend(t);
                                            break;
                                        }
                                        Some(Ok(crate::grpc::client::StreamItem::Message(msg))) => {
                                            if let Some(resp) = &mut captured_response {
                                                resp.messages.push(msg);
                                            }
                                        }
                                        _ => break,
                                    }
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
                                        &mut assertion_records,
                                        format!(
                                            "(attached to RESPONSE at line {})",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: scope_timing.as_ref(),
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                }
                            }
                            skip_next_section = true;
                        } else if section.inline_options.with_asserts && !effective_no_assert {
                            failure_reasons.push(format!(
                            "RESPONSE at line {} has 'with_asserts' but is not followed by ASSERTS",
                            section_header_line(section.start_line)
                        ));
                        }
                    }
                    SectionType::Asserts => {
                        if i >= last_request_idx.unwrap_or(usize::MAX) {
                            drop(tx.take());
                        }

                        ensure_stream_ready!();

                        if last_error_json.is_some() || last_error_message.is_some() {
                            if !effective_no_assert
                                && let SectionContent::Assertions(lines) = &section.content
                            {
                                if let Some(error_value) = &last_error_json {
                                    self.run_assertions(
                                        lines,
                                        error_value,
                                        &mut failure_reasons,
                                        &mut assertion_records,
                                        format!(
                                            "after ERROR at line {}",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: last_error_timing.as_ref(),
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                } else if let Some(error_message) = &last_error_message {
                                    let error_value = Value::String(error_message.clone());
                                    self.run_assertions(
                                        lines,
                                        &error_value,
                                        &mut failure_reasons,
                                        &mut assertion_records,
                                        format!(
                                            "after ERROR at line {}",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: last_error_timing.as_ref(),
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                }
                            }
                            continue;
                        }

                        let Some(stream) = response_stream.as_mut() else {
                            if !effective_no_assert {
                                failure_reasons.push(format!(
                                    "ASSERTS section at line {} has no active response/error context",
                                    section_header_line(section.start_line)
                                ));
                            }
                            continue;
                        };

                        let read_timeout_secs = get_timeout().unwrap_or(effective_timeout_seconds);
                        let next_item = if read_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(read_timeout_secs),
                                stream.next(),
                            )
                            .await
                            {
                                Ok(item) => item,
                                Err(_) => {
                                    failure_reasons.push(format!(
                                        "Timed out after {}s waiting for stream message for ASSERTS section at line {}",
                                        read_timeout_secs, section_header_line(section.start_line)
                                    ));
                                    transport_failure = true;
                                    response_stream = None;
                                    continue;
                                }
                            }
                        } else {
                            stream.next().await
                        };

                        match next_item {
                            Some(Ok(crate::grpc::client::StreamItem::Message(msg))) => {
                                let scope_start_ms =
                                    assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                                let scope_end_ms = start_time.elapsed().as_millis() as u64;
                                assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                                let scope_timing =
                                    assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);

                                last_message = Some(msg.clone());
                                if let Some(resp) = &mut captured_response {
                                    resp.messages.push(msg.clone());
                                }

                                let should_format_message =
                                    tracing::enabled!(tracing::Level::DEBUG) || effective_no_assert;
                                let msg_pretty = should_format_message
                                    .then(|| runner_helpers::format_json_pretty(&msg));

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
                                        &mut assertion_records,
                                        format!(
                                            "at line {}",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: scope_timing.as_ref(),
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                }
                            }
                            Some(Ok(crate::grpc::client::StreamItem::Trailers(t))) => {
                                if let Some(resp) = &mut captured_response {
                                    resp.trailers
                                        .extend(t.iter().map(|(k, v)| (k.clone(), v.clone())));
                                }
                                captured_trailers.extend(t);
                                if !effective_no_assert
                                    && let SectionContent::Assertions(lines) = &section.content
                                {
                                    let target = last_message.clone().unwrap_or(Value::Null);
                                    self.run_assertions(
                                        lines,
                                        &target,
                                        &mut failure_reasons,
                                        &mut assertion_records,
                                        format!(
                                            "(trailers) at line {}",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: None,
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                }
                            }
                            Some(Err(status)) => {
                                rpc_end = Some(std::time::Instant::now());
                                grpc_status = Some(status.code());
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
                                let error_json =
                                    super::error_handler::ErrorHandler::status_to_json(&status);
                                last_error_json = Some(error_json.clone());
                                captured_trailers.extend(status.metadata().clone());
                                if !effective_no_assert
                                    && let SectionContent::Assertions(lines) = &section.content
                                {
                                    self.run_assertions(
                                        lines,
                                        &error_json,
                                        &mut failure_reasons,
                                        &mut assertion_records,
                                        format!(
                                            "after ERROR at line {}",
                                            section_header_line(section.start_line)
                                        ),
                                        section.start_line,
                                        AssertionContext {
                                            headers: &captured_headers,
                                            trailers: &captured_trailers,
                                            timing: last_error_timing.as_ref(),
                                            variables: &*variables,
                                            protocol: protocol_str(client_protocol),
                                        },
                                    );
                                } else {
                                    println!("--- RESPONSE (Error) ---");
                                    println!("{}", status.message());
                                }
                            }
                            None => {
                                if !effective_no_assert {
                                    failure_reasons.push(format!(
                                    "Expected message for ASSERTS section at line {}, but stream ended",
                                    section_header_line(section.start_line)
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
                                                tracing::trace!(
                                                    "extract: {key} = {query} -> {val:?}"
                                                );
                                                variables.insert(key.clone(), val.clone());
                                            } else {
                                                failure_reasons.push(format!(
                                                 "Extraction failed at line {}: Query '{}' returned no results",
                                                 section_header_line(section.start_line), query
                                             ));
                                            }
                                        }
                                        Err(e) => {
                                            failure_reasons.push(format!(
                                                "Extraction error at line {}: {}",
                                                section_header_line(section.start_line),
                                                e
                                            ));
                                        }
                                    }
                                }
                            }
                        } else {
                            failure_reasons.push(format!(
                                "EXTRACT at line {} requires a previous response message",
                                section_header_line(section.start_line)
                            ));
                        }
                    }
                    SectionType::Error => {
                        if i >= last_request_idx.unwrap_or(usize::MAX) {
                            drop(tx.take());
                        }

                        if response_stream.is_none()
                            && let Some(handle) =
                                call_handle.take().and_then(AbortOnDrop::into_inner)
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
                                Ok(Err(_e)) => {
                                    let e = _e;
                                    if let Some(status) =
                                        e.downcast_ref::<apif_grpc_transport::GrpcError>()
                                    {
                                        grpc_status = Some(status.code());
                                    }
                                    if let Some(resp) = &mut captured_response {
                                        match e.downcast_ref::<apif_grpc_transport::GrpcError>() {
                                            Some(status) if status.code() != 14 => {
                                                resp.error = Some(status.message().to_string());
                                            }
                                            _ => transport_failure = true,
                                        }
                                    }
                                    let mut error_assert_target: Option<Value> = None;
                                    if !effective_no_assert {
                                        if let SectionContent::Json(expected_json) =
                                            &section.content
                                        {
                                            let mut expected = expected_json.clone();
                                            self.substitute_variables(&mut expected, variables);

                                            let (matches, got, mismatch_reason) = if let Some(
                                                status,
                                            ) =
                                                e.downcast_ref::<apif_grpc_transport::GrpcError>()
                                            {
                                                last_error_message =
                                                    Some(status.message().to_string());
                                                let actual_error_json =
                                                super::error_handler::ErrorHandler::status_to_json(
                                                    status,
                                                );
                                                last_error_json = Some(actual_error_json.clone());
                                                error_assert_target =
                                                    Some(actual_error_json.clone());
                                                captured_trailers.extend(status.metadata().clone());
                                                let status_name =
                                                    Self::grpc_code_name_from_numeric(
                                                        status.code() as i64,
                                                    )
                                                    .unwrap_or("Unknown");
                                                (
                                                super::error_handler::ErrorHandler::status_matches_expected_with_options(
                                                    status,
                                                    &expected,
                                                    section.inline_options.partial,
                                                ),
                                                format!(
                                                    "status: {}, message: \"{}\"",
                                                    status_name,
                                                    status.message()
                                                ),
                                                super::error_handler::ErrorHandler::status_mismatch_reason_with_options(
                                                    status,
                                                    &expected,
                                                    section.inline_options.partial,
                                                ),
                                            )
                                            } else {
                                                let text = e.to_string();
                                                error_assert_target =
                                                    Some(Value::String(text.clone()));
                                                (
                                                    Self::error_matches_expected(&text, &expected),
                                                    text,
                                                    None,
                                                )
                                            };

                                            if self.verbose {
                                                println!(
                                                    "[{}@{}] 🔍 gRPC error received: '{}'",
                                                    protocol_display(client_protocol),
                                                    client_address,
                                                    got
                                                );
                                                if let Some(status) = e
                                                    .downcast_ref::<apif_grpc_transport::GrpcError>(
                                                ) {
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
                                                    section_header_line(section.start_line)
                                                ));
                                                if let Some(reason) = mismatch_reason {
                                                    failure_reasons.push(format!("  - {}", reason));
                                                }
                                                if let Some(status) = e
                                                    .downcast_ref::<apif_grpc_transport::GrpcError>(
                                                ) {
                                                    let actual_json =
                                                    super::error_handler::ErrorHandler::status_to_json(
                                                        status,
                                                    );
                                                    failure_reasons.push(get_json_diff(
                                                        &expected,
                                                        &actual_json,
                                                    ));
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

                                    if Self::has_required_followup_asserts(
                                        section,
                                        sections,
                                        i,
                                        effective_no_assert,
                                        &mut failure_reasons,
                                    ) && let Some(next_section) = sections.get(i + 1)
                                        && next_section.section_type == SectionType::Asserts
                                    {
                                        if !effective_no_assert
                                            && let SectionContent::Assertions(lines) =
                                                &next_section.content
                                        {
                                            if error_assert_target.is_none()
                                                && let Some(status) = e
                                                    .downcast_ref::<apif_grpc_transport::GrpcError>(
                                                )
                                            {
                                                let actual_error_json =
                                                super::error_handler::ErrorHandler::status_to_json(
                                                    status,
                                                );
                                                last_error_json = Some(actual_error_json.clone());
                                                error_assert_target = Some(actual_error_json);
                                            }

                                            if let Some(target) = error_assert_target.as_ref() {
                                                self.run_assertions(
                                                    lines,
                                                    target,
                                                    &mut failure_reasons,
                                                    &mut assertion_records,
                                                    format!(
                                                        "(attached to ERROR at line {})",
                                                        section_header_line(section.start_line)
                                                    ),
                                                    section.start_line,
                                                    AssertionContext {
                                                        headers: &captured_headers,
                                                        trailers: &captured_trailers,
                                                        timing: last_error_timing.as_ref(),
                                                        variables: &*variables,
                                                        protocol: protocol_str(client_protocol),
                                                    },
                                                );
                                            }
                                        }
                                        skip_next_section = true;
                                    }

                                    continue;
                                }
                                Err(e) => {
                                    failure_reasons.push(format!(
                                        "Failed to join gRPC stream startup task: {}",
                                        e
                                    ));
                                    transport_failure = true;
                                    break;
                                }
                            }
                        }

                        let Some(error_stream) = response_stream.as_mut() else {
                            failure_reasons.push(format!(
                                "No response stream available for ERROR section at line {}",
                                section_header_line(section.start_line)
                            ));
                            transport_failure = true;
                            break;
                        };
                        let read_timeout_secs = get_timeout().unwrap_or(effective_timeout_seconds);
                        let next_item = if read_timeout_secs > 0 {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(read_timeout_secs),
                                error_stream.next(),
                            )
                            .await
                            {
                                Ok(item) => item,
                                Err(_) => {
                                    failure_reasons.push(format!(
                                        "Timed out after {}s waiting for stream error for ERROR section at line {}",
                                        read_timeout_secs, section_header_line(section.start_line)
                                    ));
                                    transport_failure = true;
                                    response_stream = None;
                                    break;
                                }
                            }
                        } else {
                            error_stream.next().await
                        };
                        match next_item {
                            Some(Err(status)) => {
                                rpc_end = Some(std::time::Instant::now());
                                grpc_status = Some(status.code());
                                let scope_start_ms =
                                    assertion_timing.last_message_elapsed_ms.unwrap_or(0);
                                let scope_end_ms = start_time.elapsed().as_millis() as u64;
                                assertion_timing.last_message_elapsed_ms = Some(scope_end_ms);
                                let error_scope_timing =
                                    assertion_timing.finish_scope(scope_start_ms, scope_end_ms, 1);

                                let status_message = status.message();
                                last_error_message = Some(status_message.to_string());
                                if let Some(resp) = &mut captured_response {
                                    resp.error = Some(status_message.to_string());
                                }
                                let error_json =
                                    super::error_handler::ErrorHandler::status_to_json(&status);
                                last_error_json = Some(error_json.clone());
                                last_error_timing = error_scope_timing;
                                captured_trailers.extend(status.metadata().clone());
                                let should_format_error = effective_no_assert || self.verbose;
                                let got = should_format_error.then(|| {
                                    let status_name =
                                        Self::grpc_code_name_from_numeric(status.code() as i64)
                                            .unwrap_or("Unknown");
                                    format!(
                                        "status: {}, message: \"{}\"",
                                        status_name, status_message
                                    )
                                });

                                if effective_no_assert {
                                    println!("--- RESPONSE (Error) ---");
                                    if let Some(got) = got.as_deref() {
                                        println!("{}", got);
                                    }
                                } else if self.verbose {
                                    if let Some(got) = got.as_deref() {
                                        println!(
                                            "[{}@{}] 🔍 gRPC error received: '{}'",
                                            protocol_display(client_protocol),
                                            client_address,
                                            got
                                        );
                                    }
                                    let details_json =
                                        super::error_handler::ErrorHandler::status_details_json(
                                            &status,
                                        );
                                    if details_json != Value::Null
                                        && details_json
                                            .as_array()
                                            .is_some_and(|arr| !arr.is_empty())
                                    {
                                        println!("🔍 gRPC error details: {}", details_json);
                                    }
                                }

                                if !effective_no_assert {
                                    if let SectionContent::Json(expected_json) = &section.content {
                                        let mut expected = expected_json.clone();
                                        self.substitute_variables(&mut expected, variables);

                                        if !super::error_handler::ErrorHandler::status_matches_expected_with_options(
                                        &status,
                                        &expected,
                                        section.inline_options.partial,
                                    ) {
                                        failure_reasons.push(format!(
                                            "Error mismatch at line {}:",
                                            section_header_line(section.start_line)
                                        ));
                                        if let Some(reason) =
                                            super::error_handler::ErrorHandler::status_mismatch_reason_with_options(
                                                &status,
                                                &expected,
                                                section.inline_options.partial,
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

                                    if Self::has_required_followup_asserts(
                                        section,
                                        sections,
                                        i,
                                        effective_no_assert,
                                        &mut failure_reasons,
                                    ) && let Some(next_section) = sections.get(i + 1)
                                        && next_section.section_type == SectionType::Asserts
                                        && let SectionContent::Assertions(lines) =
                                            &next_section.content
                                    {
                                        self.run_assertions(
                                            lines,
                                            &error_json,
                                            &mut failure_reasons,
                                            &mut assertion_records,
                                            format!(
                                                "(attached to ERROR at line {})",
                                                section_header_line(section.start_line)
                                            ),
                                            section.start_line,
                                            AssertionContext {
                                                headers: &captured_headers,
                                                trailers: &captured_trailers,
                                                timing: last_error_timing.as_ref(),
                                                variables: &*variables,
                                                protocol: protocol_str(client_protocol),
                                            },
                                        );
                                        skip_next_section = true;
                                    }
                                } else {
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
                                    section_header_line(section.start_line)
                                ));
                                } else {
                                    if let crate::grpc::client::StreamItem::Message(msg) = msg_item
                                    {
                                        println!("--- RESPONSE (Raw) ---");
                                        println!("{}", runner_helpers::format_json_pretty(&msg));
                                    }
                                }
                            }
                            None => {
                                if !effective_no_assert {
                                    failure_reasons.push(format!(
                                        "Expected ERROR at line {}, but stream ended successfully",
                                        section_header_line(section.start_line)
                                    ));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        drop(tx.take());

        if let Some(resp) = &mut captured_response {
            if response_stream.is_none()
                && let Some(handle) = call_handle.take().and_then(AbortOnDrop::into_inner)
            {
                match handle.await {
                    Ok(Ok((h, stream))) => {
                        resp.headers = h;
                        response_stream = Some(stream);
                    }
                    Ok(Err(e)) => {
                        failure_reasons.push(format!("Failed to start gRPC stream: {}", e));
                        transport_failure = true;
                        response_stream = None;
                    }
                    Err(e) => {
                        failure_reasons
                            .push(format!("Failed to join gRPC stream startup task: {}", e));
                        transport_failure = true;
                        response_stream = None;
                    }
                }
            }

            loop {
                let next_item = if let Some(stream) = response_stream.as_mut() {
                    if effective_timeout_seconds > 0 {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(effective_timeout_seconds),
                            stream.next(),
                        )
                        .await
                        {
                            Ok(item) => item,
                            Err(_) => {
                                failure_reasons.push(format!(
                                    "Timed out after {}s draining response stream in write mode",
                                    effective_timeout_seconds
                                ));
                                transport_failure = true;
                                None
                            }
                        }
                    } else {
                        stream.next().await
                    }
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

        if assertion_records.iter().any(|rec| rec.endpoint.is_none()) {
            let endpoint_label = format!("{}/{}", full_service, method);
            for rec in &mut assertion_records {
                if rec.endpoint.is_none() {
                    rec.endpoint = Some(endpoint_label.clone());
                }
            }
        }

        let grpc_elapsed = rpc_end.map_or_else(
            || start_time.elapsed(),
            |end| end.saturating_duration_since(start_time),
        );
        let grpc_duration = grpc_elapsed.as_millis() as u64;
        let grpc_duration_ns = grpc_elapsed.as_nanos() as u64;

        if !failure_reasons.is_empty() {
            if effective_write_mode
                && !transport_failure
                && let Some(resp) = captured_response
            {
                return Ok(TestExecutionResult::pass(Some(grpc_duration))
                    .with_call_duration_ns(grpc_duration_ns)
                    .with_response(resp)
                    .with_grpc_status(grpc_status.unwrap_or(0))
                    .with_assertions(assertion_records)
                    .dialled(&client_address)
                    .with_retried(retry_occurred));
            }

            let kind = if transport_failure {
                FailureKind::Transport
            } else {
                FailureKind::Assertion
            };
            let mut result = TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                Some(grpc_duration),
            )
            .with_call_duration_ns(grpc_duration_ns)
            .with_failure_kind(kind)
            .with_assertions(assertion_records)
            .dialled(&client_address)
            .with_retried(retry_occurred);
            if transport_failure && let Some(code) = grpc_status {
                result = result.with_grpc_status(code);
            }
            if let Some(resp) = captured_response {
                result = result.with_response(resp);
            }
            return Ok(result);
        }

        let mut result = TestExecutionResult::pass(Some(grpc_duration))
            .with_call_duration_ns(grpc_duration_ns)
            .with_grpc_status(grpc_status.unwrap_or(0))
            .with_assertions(assertion_records)
            .dialled(&client_address)
            .with_retried(retry_occurred);
        if !transport_failure && let Some(resp) = captured_response {
            result = result.with_response(resp);
        }
        Ok(result)
    }

    pub fn validate_response(
        &self,
        document: &GctfDocument,
        response: &crate::grpc::GrpcResponse,
    ) -> TestExecutionResult {
        self.response_handler.validate_document(document, response)
    }

    fn substitute_variables(&self, value: &mut Value, variables: &HashMap<String, Value>) {
        runner_helpers::substitute_variables(value, variables);
    }

    fn dialled_origin(address: Option<&str>, url: &str) -> String {
        if let Some(address) = address.map(str::trim).filter(|a| !a.is_empty()) {
            return address.to_string();
        }
        match url.split_once("://") {
            Some((scheme, rest)) => {
                let host = rest.split('/').next().unwrap_or(rest);
                format!("{scheme}://{host}")
            }
            None => url.split('/').next().unwrap_or(url).to_string(),
        }
    }

    fn http_timeout_seconds(document: &GctfDocument, default_seconds: u64) -> u64 {
        document
            .sections
            .iter()
            .filter_map(|s| s.get_timeout())
            .next()
            .or_else(|| {
                document
                    .get_options()
                    .as_ref()
                    .and_then(|o| o.get("timeout"))
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .filter(|v| *v > 0)
            })
            .unwrap_or(default_seconds)
    }

    async fn run_http(
        &self,
        document: &GctfDocument,
        variables: &mut HashMap<String, Value>,
        chain_address: Option<&str>,
    ) -> TestExecutionResult {
        use apif_http_transport as http;

        let Some((method, path)) = document.parse_http_endpoint() else {
            return TestExecutionResult::fail(
                "ENDPOINT must be a method and a path, like `POST /v1/users`".to_string(),
                None,
            );
        };

        let timeout_seconds = Self::http_timeout_seconds(document, self.timeout_seconds);

        let address = match &self.address_override {
            Some(a) => Some(a.clone()),
            None => document
                .get_address(None)
                .filter(|a| !a.trim().is_empty())
                .or_else(|| chain_address.map(str::to_string))
                .or_else(|| self.env_address.clone())
                .filter(|a| !a.trim().is_empty()),
        };
        let path = runner_helpers::interpolate_variables(&path, variables).unwrap_or(path);
        let address =
            address.map(|a| runner_helpers::interpolate_variables(&a, variables).unwrap_or(a));
        let url = http::url_for(address.as_deref(), &path);

        let mut headers = HashMap::new();
        if let Some(declared) = document.get_request_headers() {
            for (key, value) in declared {
                let value =
                    runner_helpers::interpolate_variables(&value, variables).unwrap_or(value);
                headers.insert(key, value);
            }
        }

        let body = document
            .first_section(SectionType::Request)
            .filter(|section| !section.get_skip())
            .and_then(|section| match &section.content {
                SectionContent::Json(value) => {
                    let mut value = value.clone();
                    self.substitute_variables(&mut value, variables);
                    Some(runner_helpers::format_json_pretty(&value))
                }
                SectionContent::Single(text) => Some(
                    runner_helpers::interpolate_variables(text, variables)
                        .unwrap_or_else(|| text.clone()),
                ),
                SectionContent::Empty => None,
                _ => {
                    let raw = section.raw_content.trim();
                    (!raw.is_empty()).then(|| {
                        runner_helpers::interpolate_variables(raw, variables)
                            .unwrap_or_else(|| raw.to_string())
                    })
                }
            });

        if self.dry_run {
            return TestExecutionResult::pass(Some(0));
        }

        let answer = match http::send(http::HttpCall {
            method: method.clone(),
            url: url.clone(),
            headers,
            body,
            timeout: std::time::Duration::from_secs(timeout_seconds),
        })
        .await
        {
            Ok(answer) => answer,
            Err(message) => {
                return TestExecutionResult::fail(message, None)
                    .dialled(&Self::dialled_origin(address.as_deref(), &url))
                    .with_failure_kind(FailureKind::Transport);
            }
        };

        let mut failure_reasons: Vec<String> = Vec::new();
        let mut assertion_records: Vec<apif_state::AssertionRecord> = Vec::new();
        let trailers: HashMap<String, String> = HashMap::new();
        let timing = AssertionTiming {
            elapsed_ms: answer.duration_ms,
            total_elapsed_ms: answer.duration_ms,
            scope_message_count: 1,
            scope_index: 0,
        };

        for section in &document.sections {
            if section.get_skip() {
                continue;
            }
            match section.section_type {
                SectionType::Response => {
                    let expected = match &section.content {
                        SectionContent::Json(value) => value.clone(),
                        SectionContent::Single(text) => Value::String(text.clone()),
                        SectionContent::Empty => continue,
                        _ => Value::String(section.raw_content.trim().to_string()),
                    };
                    let diffs =
                        JsonComparator::compare(&answer.body, &expected, &section.inline_options);
                    assertion_records.push(response_record(
                        section,
                        &diffs,
                        &expected,
                        &answer.body,
                    ));
                    if !diffs.is_empty() {
                        self.append_response_diffs(
                            diffs,
                            section_header_line(section.start_line),
                            &expected,
                            &answer.body,
                            &mut failure_reasons,
                        );
                    }
                }
                SectionType::Asserts => {
                    if self.no_assert {
                        continue;
                    }
                    if let SectionContent::Assertions(lines) = &section.content {
                        self.run_assertions(
                            lines,
                            &answer.body,
                            &mut failure_reasons,
                            &mut assertion_records,
                            "ASSERTS".to_string(),
                            section.start_line,
                            AssertionContext {
                                headers: &answer.headers,
                                trailers: &trailers,
                                timing: Some(&timing),
                                variables,
                                protocol: "http",
                            },
                        );
                    }
                }
                SectionType::Extract => {
                    if let SectionContent::Extract(extractions) = &section.content {
                        for (key, query) in extractions {
                            match self.assertion_engine.query(query, &answer.body) {
                                Ok(results) => match results.first() {
                                    Some(value) => {
                                        variables.insert(key.clone(), value.clone());
                                    }
                                    None => failure_reasons.push(format!(
                                        "Extraction failed at line {}: Query '{}' returned no results",
                                        section_header_line(section.start_line), query
                                    )),
                                },
                                Err(e) => failure_reasons.push(format!(
                                    "Extraction error at line {}: {}",
                                    section_header_line(section.start_line), e
                                )),
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut captured = crate::grpc::GrpcResponse::new();
        captured.headers = answer.headers.clone();
        captured.messages = vec![answer.body.clone()];

        let mut result = if failure_reasons.is_empty() {
            TestExecutionResult::pass(Some(answer.duration_ms))
        } else {
            TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                Some(answer.duration_ms),
            )
        };
        for record in &mut assertion_records {
            if record.endpoint.is_none() {
                record.endpoint = Some(format!("{method} {path}"));
            }
        }
        result.assertions = assertion_records;
        result.grpc_status = Some(answer.status as u32);
        if self.capture_exchange {
            result.captured_response = Some(captured);
        }
        result.meta = crate::commands::run::extract_test_meta(document);
        result.dialled_address = Some(Self::dialled_origin(address.as_deref(), &url));
        result
    }

    #[expect(clippy::too_many_arguments)]
    fn run_assertions(
        &self,
        lines: &[String],
        target_value: &Value,
        failure_reasons: &mut Vec<String>,
        assertion_records: &mut Vec<apif_state::AssertionRecord>,
        context: String,
        start_line: usize,
        assertion_context: AssertionContext<'_>,
    ) {
        let result = self.assertion_handler.evaluate_assertions_for_section(
            lines,
            target_value,
            assertion_context.headers,
            assertion_context.trailers,
            &context,
            start_line,
            assertion_context.timing,
            assertion_context.variables,
            assertion_context.protocol,
        );

        if !result.passed {
            failure_reasons.extend(result.failure_messages);
        }
        assertion_records.extend(result.records);
    }

    fn append_response_diffs(
        &self,
        diffs: Vec<crate::assert::AssertionResult>,
        section_line: usize,
        expected: &Value,
        actual: &Value,
        failure_reasons: &mut Vec<String>,
    ) {
        failure_reasons.push(format!("Response mismatch at line {}:", section_line));
        for diff in diffs {
            match diff {
                crate::assert::AssertionResult::Fail {
                    message,
                    expected: exp,
                    actual: act,
                    hint: _,
                } => {
                    let mut msg = format!("  - {}", message);
                    if let (Some(e), Some(a)) = (exp, act) {
                        msg.push_str(&format!("\n      Expected: {}\n      Actual:   {}", e, a));
                    }
                    failure_reasons.push(msg);
                }
                crate::assert::AssertionResult::Error(m) => {
                    failure_reasons.push(format!("  - Error: {}", m));
                }
                _ => {}
            }
        }
        failure_reasons.push(get_json_diff(expected, actual));
    }

    fn log_response_message(msg: &Value, verbose: bool, protocol: &str, addr: &str) {
        let should_format = tracing::enabled!(tracing::Level::DEBUG) || verbose;
        if !should_format {
            return;
        }
        let pretty = runner_helpers::format_json_pretty(msg);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[{}@{}] Received Response:\n{}", protocol, addr, pretty);
        }
        if verbose {
            println!("--- RESPONSE (Raw) ---\n{}", pretty);
        }
    }

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
        let full_service = runner_helpers::full_service_name(package, service);
        println!("   Endpoint: {} / {}", full_service, method);
        println!();

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
                        let json_str = runner_helpers::format_json_pretty(json);
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
                            let json_str = runner_helpers::format_json_pretty(json);
                            println!(
                                "   ↤ RESPONSE (Line {}):{}",
                                section_header_line(section.start_line),
                                with_asserts
                            );
                            println!("     {}", json_str.replace('\n', "\n     "));
                        }
                        SectionContent::JsonLines(values) => {
                            println!(
                                "   ↤ RESPONSE (Line {}, {} messages):{}",
                                section_header_line(section.start_line),
                                values.len(),
                                with_asserts
                            );
                            for value in values {
                                let json_str = runner_helpers::format_json_pretty(value);
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
                        let json_str = runner_helpers::format_json_pretty(json);
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
                SectionType::Bench | SectionType::Meta | SectionType::Dataset => {}
            }
        }

        let tls_defaults = runner_helpers::tls_env_defaults();
        if let Some(tls_config) = document.get_tls_config_with_defaults(tls_defaults) {
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
                .is_some_and(|s| runner_helpers::parse_bool_flag(s))
            {
                println!("   Insecure Skip Verify: true");
            }
        }

        if let Some(proto_config) = document.get_proto_config() {
            println!();
            println!("📄 Proto Configuration:");
            if let Some(descriptor) = proto_config.get("descriptor") {
                println!("   Descriptor: {}", descriptor);
            }
            if let Some(files) = proto_config.get("files") {
                println!("   Proto Files: {}", files);
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

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn an_http_document_that_does_not_name_a_method_says_so_before_dialling() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\n/v1/users\n\n--- ASSERTS ---\n.ok\n",
            "t.httf",
        )
        .expect("parses");
        let result = TestRunner::new(false, 30, false, false, false, None)
            .run_test(&doc)
            .await
            .expect("runs");
        match result.status {
            TestExecutionStatus::Fail(message) => {
                assert!(message.contains("POST /v1/users"), "{message}")
            }
            TestExecutionStatus::Pass => panic!("an endpoint with no method cannot pass"),
        }
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_result_says_where_the_call_went() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\n\r\n{\"name\":\"Ada\"}",
            )
            .await;
        });

        let src = format!("--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /x\n");
        let document = crate::parser::parse_content_with_recovery(&src, "where.httf").document;
        let runner = TestRunner::new(false, 5, false, false, false, None);
        let mut variables = HashMap::new();
        let result = runner.run_http(&document, &mut variables, None).await;

        assert_eq!(
            result.dialled_address.as_deref(),
            Some(format!("http://{addr}").as_str()),
            "the address it was pointed at, not the URL it built from it"
        );
        assert_eq!(
            TestRunner::dialled_origin(None, "https://api.example.com/v1/users"),
            "https://api.example.com"
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn a_skipped_section_is_not_checked_for_an_http_file() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\n\r\n{\"name\":\"Ada\"}",
            )
            .await;
        });

        let src = format!(
            "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /x\n\n#[skip]\n--- ASSERTS ---\n.name == \"Grace\"\n"
        );
        let document = crate::parser::parse_content_with_recovery(&src, "skipped.httf").document;
        let runner = TestRunner::new(false, 5, false, false, false, None);
        let mut variables = HashMap::new();
        let result = runner.run_http(&document, &mut variables, None).await;

        assert!(
            matches!(result.status, TestExecutionStatus::Pass),
            "a skipped ASSERTS must not be checked: {:?}",
            result.status
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn an_http_check_names_the_request_it_checked() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\n\r\n{\"name\":\"Ada\"}",
            )
            .await;
        });

        let src = format!(
            "--- ADDRESS ---\nhttp://{addr}\n\n--- ENDPOINT ---\nGET /v1/users/{{{{who}}}}\n\n--- ASSERTS ---\n.name == \"Ada\"\n"
        );
        let document = crate::parser::parse_content_with_recovery(&src, "named.httf").document;
        let runner = TestRunner::new(false, 5, false, false, false, None);
        let mut variables = HashMap::new();
        variables.insert("who".to_string(), json!("7"));
        let result = runner.run_http(&document, &mut variables, None).await;

        assert_eq!(result.assertions.len(), 1, "{result:?}");
        assert_eq!(
            result.assertions[0].endpoint.as_deref(),
            Some("GET /v1/users/7"),
        );
    }

    #[test]
    fn an_http_file_is_bounded_by_its_own_attribute_first() {
        let doc =
            |body: &str| crate::parser::parse_content_with_recovery(body, "waits.httf").document;

        let attributed = doc(
            "--- ENDPOINT ---\nGET /v1/users\n\n--- OPTIONS ---\ntimeout: 30\n\n#[timeout(5)]\n--- REQUEST ---\n{}\n",
        );
        assert_eq!(
            TestRunner::http_timeout_seconds(&attributed, 60),
            5,
            "the gRPC path reads the attribute first and this one read the OPTIONS line alone"
        );

        let options_only = doc("--- ENDPOINT ---\nGET /v1/users\n\n--- OPTIONS ---\ntimeout: 30\n");
        assert_eq!(TestRunner::http_timeout_seconds(&options_only, 60), 30);

        let silent = doc("--- ENDPOINT ---\nGET /v1/users\n");
        assert_eq!(TestRunner::http_timeout_seconds(&silent, 60), 60);

        let zero = doc("--- ENDPOINT ---\nGET /v1/users\n\n--- OPTIONS ---\ntimeout: 0\n");
        assert_eq!(TestRunner::http_timeout_seconds(&zero, 60), 60);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_dry_run_of_an_http_file_sends_nothing() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n.ok\n",
            "t.httf",
        )
        .expect("parses");
        let result = TestRunner::new(true, 30, false, false, false, None)
            .run_test(&doc)
            .await
            .expect("runs");
        assert!(matches!(result.status, TestExecutionStatus::Pass));
    }

    #[test]
    fn the_plan_of_an_http_file_is_http() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n",
            "t.httf",
        )
        .expect("parses");
        let plan = ExecutionPlan::from_document(&doc);
        assert_eq!(plan.connection.backend, "https");
        assert_eq!(plan.summary.rpc_mode_name, "POST");
        assert_eq!(plan.target.endpoint, "POST /v1/users");
        assert_eq!(plan.target.service, None);
        assert_eq!(plan.target.method, None);
    }

    #[test]
    fn a_plain_http_file_says_http_not_https() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nlocalhost:8080\n\n--- ENDPOINT ---\nGET /health\n\n--- ASSERTS ---\n.ok\n",
            "t.httf",
        )
        .expect("parses");
        assert_eq!(
            ExecutionPlan::from_document(&doc).connection.backend,
            "http"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn the_plan_names_the_transport_the_file_selects() {
        let plain = parse(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
        );
        assert_eq!(
            ExecutionPlan::from_document(&plain).connection.backend,
            "grpc"
        );

        let web = parse(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\nprotocol: grpc-web\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
        );
        assert_eq!(
            ExecutionPlan::from_document(&web).connection.backend,
            "grpc-web"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_request_checked_only_by_asserts_is_still_unary() {
        let doc = parse(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
        );
        let plan = ExecutionPlan::from_document(&doc);
        assert_eq!(plan.summary.rpc_mode_name, "Unary");
    }

    fn runner() -> TestRunner {
        TestRunner::new(false, 30, false, false, false, None)
    }

    fn parse(text: &str) -> crate::parser::GctfDocument {
        parse_named(text, "t.gctf")
    }

    fn parse_named(text: &str, name: &str) -> crate::parser::GctfDocument {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, text).expect("write");
        crate::parser::parse_with_recovery(&path).document
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn preparation_matches_the_document() {
        let doc = parse(
            "--- ADDRESS ---\n127.0.0.1:9\n\n--- ENDPOINT ---\npkg.Svc/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner().prepare(&doc);
        let prep = prepared.0[0].as_ref().expect("document is preparable");
        assert_eq!(prep.address, "127.0.0.1:9");
        assert_eq!(prep.package, "pkg");
        assert_eq!(prep.service, "Svc");
        assert_eq!(prep.method, "Method");
        assert_eq!(prep.full_service, "pkg.Svc");
        assert_eq!(prep.timeout_seconds, 30);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn an_unusable_timeout_is_not_prepared() {
        let doc = parse(
            "--- ENDPOINT ---\npkg.Svc/Method\n\n--- OPTIONS ---\ntimeout: not-a-number\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner().prepare(&doc);
        assert!(
            prepared.0[0].is_none(),
            "a bad timeout must fall back to the reporting path"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_document_without_an_endpoint_is_not_prepared() {
        let doc = parse("--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n");
        let prepared = runner().prepare(&doc);
        assert!(prepared.0[0].is_none());
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn an_http_test_with_no_address_says_which_target_is_missing() {
        let doc = parse_named(
            "--- ENDPOINT ---\nGET /data.json\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        );
        let result = TestRunner::new(false, 5, false, false, false, None)
            .run_test_with_variables(&doc, HashMap::new())
            .await
            .expect("runs");
        match result.status {
            TestExecutionStatus::Fail(message) => {
                assert!(message.contains("no address for /data.json"), "{message}");
                assert!(message.contains("ADDRESS"), "{message}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_later_step_inherits_the_address_the_chain_started_with() {
        let doc = parse(
            "--- ADDRESS ---\n127.0.0.1:9\n\n--- ENDPOINT ---\npkg.Svc/A\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\npkg.Svc/B\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner()
            .with_env_address("elsewhere:1".to_string())
            .prepare(&doc);
        assert_eq!(
            prepared.0[0].as_ref().expect("preparable").address,
            "127.0.0.1:9"
        );
        assert_eq!(
            prepared.0[1].as_ref().expect("preparable").address,
            "127.0.0.1:9"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_step_that_names_its_own_address_keeps_it_and_passes_it_on() {
        let doc = parse(
            "--- ADDRESS ---\n127.0.0.1:9\n\n--- ENDPOINT ---\npkg.Svc/A\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ADDRESS ---\n127.0.0.1:11\n\n--- ENDPOINT ---\npkg.Svc/B\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\npkg.Svc/C\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner().prepare(&doc);
        assert_eq!(
            prepared.0[1].as_ref().expect("preparable").address,
            "127.0.0.1:11"
        );
        assert_eq!(
            prepared.0[2].as_ref().expect("preparable").address,
            "127.0.0.1:11"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_chain_with_no_address_still_reads_the_environment() {
        let doc = parse(
            "--- ENDPOINT ---\npkg.Svc/A\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\npkg.Svc/B\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner()
            .with_env_address("elsewhere:1".to_string())
            .prepare(&doc);
        assert_eq!(
            prepared.0[1].as_ref().expect("preparable").address,
            "elsewhere:1"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn an_address_override_wins_over_the_document() {
        let doc = parse(
            "--- ADDRESS ---\n127.0.0.1:9\n\n--- ENDPOINT ---\npkg.Svc/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
        );
        let prepared = runner()
            .with_address_override("127.0.0.1:10".to_string())
            .prepare(&doc);
        assert_eq!(
            prepared.0[0].as_ref().expect("preparable").address,
            "127.0.0.1:10"
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn dropping_the_guard_cancels_the_task() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let guard = AbortOnDrop(Some(tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            flag.store(true, Ordering::SeqCst);
        })));
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(!ran.load(Ordering::SeqCst), "the task kept running");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn taking_the_handle_leaves_the_task_alive() {
        let guard = AbortOnDrop(Some(tokio::spawn(async { 7u8 })));
        let handle = guard.into_inner().expect("handle is present once");
        assert_eq!(handle.await.expect("task must finish"), 7);
    }
    use crate::polyfill::runtime;
    use serde_json::json;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn a_skipped_request_leaves_the_file_as_one_with_no_request() {
        let skipped = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\npkg.Svc/M\n\n#[skip]\n--- REQUEST ---\n{\"a\": 1}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        )
        .expect("parses");
        let sends = |doc: &crate::parser::GctfDocument| {
            doc.sections
                .iter()
                .any(|s| s.section_type == SectionType::Request && !s.get_skip())
        };
        assert!(
            !sends(&skipped),
            "a skipped REQUEST is not a message to send"
        );

        let written = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\npkg.Svc/M\n\n--- REQUEST ---\n{\"a\": 1}\n\n--- RESPONSE ---\n{}\n",
            "t.gctf",
        )
        .expect("parses");
        assert!(sends(&written));
    }

    #[test]
    fn infer_rpc_mode_counts_jsonlines_request_messages_not_sections() {
        let content = r#"--- ENDPOINT ---
chat.ChatService/SendMessages

--- REQUEST ---
{ "text": "one" }
{ "text": "two" }
{ "text": "three" }

--- RESPONSE ---
{ "count": 3 }
"#;
        let doc = crate::parser::parse_gctf_from_str(content, "test.gctf").expect("valid document");
        assert_eq!(
            infer_rpc_mode_for_section_types(&doc),
            RpcModeInfo::ClientStreaming
        );
    }

    #[test]
    fn chain_accumulator_carries_last_captured_response_through() {
        let mut acc = ChainAccumulator::default();

        let first =
            TestExecutionResult::pass(Some(10)).with_response(crate::grpc::GrpcResponse::new());
        assert!(!acc.absorb(first));

        let mut second_response = crate::grpc::GrpcResponse::new();
        second_response.messages.push(json!({"ok": true}));
        let second = TestExecutionResult::pass(Some(20)).with_response(second_response);
        assert!(!acc.absorb(second));

        let result = acc.into_result();
        assert_eq!(result.call_duration_ms, Some(30));
        let resp = result
            .captured_response
            .expect("last document's response must survive chain aggregation");
        assert_eq!(resp.messages, vec![json!({"ok": true})]);
    }

    #[test]
    fn chain_accumulator_stops_on_first_failure_and_keeps_its_response() {
        let mut acc = ChainAccumulator::default();

        let mut failing_response = crate::grpc::GrpcResponse::new();
        failing_response.error = Some("boom".to_string());
        let failing = TestExecutionResult::fail("assertion mismatch".to_string(), Some(5))
            .with_response(failing_response);
        assert!(acc.absorb(failing), "a Fail result must stop the chain");

        let result = acc.into_result();
        assert!(
            matches!(result.status, TestExecutionStatus::Fail(ref m) if m == "assertion mismatch")
        );
        assert_eq!(
            result.captured_response.and_then(|r| r.error),
            Some("boom".to_string())
        );
    }

    #[test]
    fn chain_accumulator_aggregates_assertions_across_documents() {
        let mut acc = ChainAccumulator::default();

        let record = apif_state::AssertionRecord {
            line: 1,
            expression: ".id == 1".to_string(),
            passed: true,
            elapsed_ms: 0,
            message: None,
            endpoint: None,
            expected: None,
            actual: None,
            hint: None,
        };
        acc.absorb(TestExecutionResult::pass(None).with_assertions(vec![record.clone()]));
        acc.absorb(TestExecutionResult::pass(None).with_assertions(vec![record.clone()]));

        assert_eq!(acc.into_result().assertions.len(), 2);
    }

    #[test]
    fn chain_accumulator_retried_is_sticky_across_documents() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(None).with_retried(true));
        acc.absorb(TestExecutionResult::pass(None).with_retried(false));
        assert!(acc.into_result().retried, "retried must stay true");
    }

    #[test]
    fn chain_accumulator_not_retried_when_no_document_retried() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(None).with_retried(false));
        assert!(!acc.into_result().retried);
    }

    #[test]
    fn chain_accumulator_collects_per_document_durations_in_order() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(Some(50)));
        acc.absorb(TestExecutionResult::pass(Some(30)));
        let result = acc.into_result();
        assert_eq!(result.document_durations_ms, vec![50, 30]);
        assert_eq!(result.call_duration_ms, Some(80));
    }

    #[test]
    fn chain_accumulator_missing_duration_recorded_as_zero() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(None));
        assert_eq!(acc.into_result().document_durations_ms, vec![0]);
    }

    #[test]
    fn runner_new() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        assert!(!runner.dry_run);
        assert_eq!(runner.timeout_seconds, 30);
        assert!(!runner.no_assert);
        assert!(!runner.write_mode);
        assert!(!runner.verbose);
    }

    #[test]
    fn runner_with_dry_run() {
        let runner = TestRunner::new(true, 30, false, false, false, None);
        assert!(runner.dry_run);
    }

    #[test]
    fn runner_with_timeout() {
        let runner = TestRunner::new(false, 60, false, false, false, None);
        assert_eq!(runner.timeout_seconds, 60);
    }

    #[test]
    fn runner_with_no_assert() {
        let runner = TestRunner::new(false, 30, true, false, false, None);
        assert!(runner.no_assert);
    }

    #[test]
    fn runner_with_write_mode() {
        let runner = TestRunner::new(false, 30, false, true, false, None);
        assert!(runner.write_mode);
    }

    #[test]
    fn parse_bool_flag_truthy_values() {
        assert!(runner_helpers::parse_bool_flag("true"));
        assert!(runner_helpers::parse_bool_flag("1"));
        assert!(runner_helpers::parse_bool_flag("YES"));
        assert!(runner_helpers::parse_bool_flag("on"));
    }

    #[test]
    fn parse_bool_flag_falsy_values() {
        assert!(!runner_helpers::parse_bool_flag("false"));
        assert!(!runner_helpers::parse_bool_flag("0"));
        assert!(!runner_helpers::parse_bool_flag("off"));
        assert!(!runner_helpers::parse_bool_flag(""));
    }

    #[test]
    fn parse_compression_option_from_options() {
        let mut options = crate::parser::OrderedStringMap::new();
        options.insert("compression".to_string(), "gzip".to_string());

        assert_eq!(
            runner_helpers::parse_compression_option(&options),
            Some(crate::grpc::CompressionMode::Gzip)
        );
    }

    #[test]
    fn parse_compression_option_none_from_options() {
        let mut options = crate::parser::OrderedStringMap::new();
        options.insert("compression".to_string(), "none".to_string());

        assert_eq!(
            runner_helpers::parse_compression_option(&options),
            Some(crate::grpc::CompressionMode::None)
        );
    }

    #[test]
    fn parse_compression_option_fallback_to_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_COMPRESSION, "gzip");
        }

        let mut options = crate::parser::OrderedStringMap::new();
        options.insert("compression".to_string(), "invalid".to_string());
        assert_eq!(runner_helpers::parse_compression_option(&options), None);

        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_COMPRESSION);
        }
    }

    #[test]
    fn resolve_tls_path_from_env_uses_cwd() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let cwd = std::env::current_dir().unwrap();
        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = runner_helpers::resolve_tls_path("certs/ca.crt", true, document_path);
        assert_eq!(Path::new(&resolved), cwd.join("certs/ca.crt"));
    }

    #[test]
    fn resolve_tls_path_from_env_without_fs_capability_returns_relative() {
        if runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = runner_helpers::resolve_tls_path("certs/ca.crt", true, document_path);
        assert_eq!(resolved, "certs/ca.crt");
    }

    #[test]
    fn resolve_tls_path_from_document_uses_document_dir() {
        let document_path = Path::new("tests/fixtures/sample.gctf");
        let resolved = runner_helpers::resolve_tls_path("certs/ca.crt", false, document_path);
        assert_eq!(
            Path::new(&resolved),
            Path::new("tests/fixtures").join("certs").join("ca.crt")
        );
    }

    #[test]
    fn tls_env_defaults_uses_grpctestify_prefix() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE, "/tmp/ca.pem");
            std::env::set_var(
                crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE,
                "/tmp/cert.pem",
            );
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE, "/tmp/key.pem");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME, "localhost");
        }

        let defaults = runner_helpers::read_tls_env_defaults();
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
    fn tls_env_defaults_ignores_empty_values() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE, "");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE, "   ");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE, "");
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME, " ");
        }

        let defaults = runner_helpers::read_tls_env_defaults();
        assert!(defaults.is_empty());

        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CA_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_CERT_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_KEY_FILE);
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_TLS_SERVER_NAME);
        }
    }

    #[test]
    fn runner_with_verbose() {
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
    fn error_matches_expected_message() {
        let expected = json!({
            "message": "Can't find stub",
            "code": 5
        });
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn error_matches_expected_code() {
        let expected = json!({
            "code": 5
        });
        let error_text = "status: NotFound, message: \"error\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn error_matches_expected_wrong_code() {
        let expected = json!({
            "code": 3
        });
        let error_text = "status: NotFound, message: \"error\"";
        assert!(!TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn error_matches_expected_wrong_message() {
        let expected = json!({
            "message": "Different error"
        });
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(!TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn error_matches_expected_string() {
        let expected = json!("Can't find stub");
        let error_text = "status: NotFound, message: \"Can't find stub\"";
        assert!(TestRunner::error_matches_expected(error_text, &expected));
    }

    #[test]
    fn full_service_name() {
        assert_eq!(
            runner_helpers::full_service_name("package", "Service"),
            "package.Service"
        );
        assert_eq!(runner_helpers::full_service_name("", "Service"), "Service");
    }

    #[test]
    fn substitute_variables_exact_match_preserves_type() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("{{ count }}");
        let mut vars = HashMap::new();
        vars.insert("count".to_string(), json!(42));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!(42));
    }

    #[test]
    fn substitute_variables_interpolation_single_pass() {
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
    fn substitute_variables_keeps_unknown_placeholder() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("hello {{known}} and {{unknown}}");
        let mut vars = HashMap::new();
        vars.insert("known".to_string(), json!("world"));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!("hello world and {{unknown}}"));
    }

    #[test]
    fn response_record_line_is_the_editor_line() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionSpan};

        let section = Section {
            section_type: crate::parser::ast::SectionType::Response,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 11,
            end_line: 14,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let record = response_record(&section, &[], &json!({}), &json!({}));
        assert_eq!(record.line, 12);
    }

    #[test]
    fn with_asserts_failure_names_the_editor_line() {
        use crate::parser::ast::{
            InlineOptions, Section, SectionContent, SectionSpan, SectionType,
        };

        let section = Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({})),
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: String::new(),
            start_line: 6,
            end_line: 8,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let mut reasons = Vec::new();
        let ok = TestRunner::has_required_followup_asserts(
            &section,
            std::slice::from_ref(&section),
            0,
            false,
            &mut reasons,
        );
        assert!(!ok);
        assert_eq!(reasons.len(), 1);
        assert!(
            reasons[0].contains("at line 7"),
            "message names the parser's line, not the editor's: {}",
            reasons[0]
        );
    }

    #[test]
    fn plan_names_the_address_line_as_written() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- ASSERTS ---\n.ok == true\n",
            "plan.gctf",
        )
        .expect("parses");
        let plan = ExecutionPlan::from_document(&doc);
        assert_eq!(plan.connection.source, "ADDRESS section [line 1]");
    }

    #[test]
    fn test_expected_values_for_response_section() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionSpan};

        let section = Section {
            section_type: crate::parser::ast::SectionType::Response,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], json!({"key": "value"}));
    }

    #[test]
    fn expected_values_for_json_lines() {
        use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionSpan};

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
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn expected_values_for_other_section() {
        use crate::parser::ast::{
            InlineOptions, Section, SectionContent, SectionSpan, SectionType,
        };

        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let values = TestRunner::expected_values_for_response_section(&section);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], json!({"key": "value"}));
    }

    #[test]
    fn metadata_map_to_hashmap_extracts_ascii_values() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "code".to_string(),
            "EXTERNAL_SERVICE_ERROR_CODE".to_string(),
        );
        metadata.insert(
            "message".to_string(),
            "External service error message".to_string(),
        );

        let trailers = metadata;
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
    fn assertion_scope_timing_single_message_scope() {
        let mut timing = AssertionScopeTimingState::default();

        let first = timing.finish_scope(0, 12, 1).unwrap();

        assert_eq!(first.elapsed_ms, 12);
        assert_eq!(first.total_elapsed_ms, 12);
        assert_eq!(first.scope_message_count, 1);
        assert_eq!(first.scope_index, 1);
    }

    #[test]
    fn assertion_scope_timing_batch_scope_uses_full_section_window() {
        let mut timing = AssertionScopeTimingState::default();

        let batch = timing.finish_scope(0, 27, 2).unwrap();

        assert_eq!(batch.elapsed_ms, 27);
        assert_eq!(batch.total_elapsed_ms, 27);
        assert_eq!(batch.scope_message_count, 2);
        assert_eq!(batch.scope_index, 1);
    }

    #[test]
    fn assertion_scope_timing_accumulates_total_duration() {
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

    #[test]
    fn has_required_followup_asserts_for_error_requires_adjacent_asserts() {
        use crate::parser::ast::{
            InlineOptions, Section, SectionContent, SectionSpan, SectionType,
        };

        let error = Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: "".to_string(),
            start_line: 12,
            end_line: 12,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };
        let sections = vec![error.clone()];
        let mut failures = Vec::new();

        let has_followup =
            TestRunner::has_required_followup_asserts(&error, &sections, 0, false, &mut failures);

        assert!(!has_followup);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("ERROR at line 13 has 'with_asserts'"));
    }

    #[test]
    fn has_required_followup_asserts_for_error_accepts_adjacent_asserts() {
        use crate::parser::ast::{
            InlineOptions, Section, SectionContent, SectionSpan, SectionType,
        };

        let error = Section {
            section_type: SectionType::Error,
            content: SectionContent::Empty,
            inline_options: InlineOptions {
                with_asserts: true,
                ..InlineOptions::default()
            },
            raw_content: "".to_string(),
            start_line: 20,
            end_line: 20,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };
        let asserts = Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".code == 5".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".code == 5".to_string(),
            start_line: 21,
            end_line: 21,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };
        let sections = vec![error.clone(), asserts];
        let mut failures = Vec::new();

        let has_followup =
            TestRunner::has_required_followup_asserts(&error, &sections, 0, false, &mut failures);

        assert!(has_followup);
        assert!(failures.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn error_assertions_evaluate_against_error_json_object() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let target = json!({
            "code": 5,
            "message": "resource not found in backend",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "NOT_FOUND"
                }
            ]
        });
        let lines = vec![
            ".code == 5".to_string(),
            ".message contains \"not found\"".to_string(),
            ".details[0][\"@type\"] == \"type.googleapis.com/google.rpc.ErrorInfo\"".to_string(),
        ];
        let mut failures = Vec::new();
        let mut assertion_records = Vec::new();
        let headers: HashMap<String, String> = HashMap::new();
        let trailers: HashMap<String, String> = HashMap::new();

        runner.run_assertions(
            &lines,
            &target,
            &mut failures,
            &mut assertion_records,
            "(attached to ERROR at line 1)".to_string(),
            1,
            AssertionContext {
                headers: &headers,
                trailers: &trailers,
                timing: None,
                variables: &HashMap::new(),
                protocol: "grpc",
            },
        );

        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert_eq!(assertion_records.len(), 3);
        assert!(assertion_records.iter().all(|r| r.passed));
    }

    #[test]
    fn a_group_costs_its_slowest_step_and_reports_all_of_them() {
        let mut acc = ChainAccumulator::default();
        let mut failed = TestExecutionResult::pass(Some(40));
        failed.status = TestExecutionStatus::Fail("the first one".to_string());
        let mut also_failed = TestExecutionResult::pass(Some(90));
        also_failed.status = TestExecutionStatus::Fail("the second one".to_string());

        let stop = acc.absorb_group(vec![
            failed,
            also_failed,
            TestExecutionResult::pass(Some(50)),
        ]);

        assert!(stop, "the chain stops after the group");
        assert_eq!(
            acc.document_durations_ms,
            vec![40, 90, 50],
            "every step is reported"
        );
        assert_eq!(
            acc.total_duration_ms as u64, 90,
            "the group cost its slowest step"
        );
        match acc.into_result().status {
            TestExecutionStatus::Fail(said) => {
                assert_eq!(
                    said, "the first one",
                    "the file's order decides which failure is the chain's"
                );
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_group_that_all_passes_carries_on() {
        let mut acc = ChainAccumulator::default();
        let stop = acc.absorb_group(vec![
            TestExecutionResult::pass(Some(10)),
            TestExecutionResult::pass(Some(20)),
        ]);
        assert!(!stop);
        assert_eq!(acc.total_duration_ms as u64, 20);
    }

    #[test]
    fn a_step_dials_the_address_of_its_own_transport() {
        let content = "--- ADDRESS ---\nhttp://api.test\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n\n--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\nauth.v1.Auth/Login\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- ENDPOINT ---\nGET /v1/orders\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parser::parse_gctf_from_str(content, "checkout.apif").unwrap();

        assert_eq!(
            chain_addresses(&doc),
            vec![
                Some("http://api.test".to_string()),
                Some("127.0.0.1:4770".to_string()),
                Some("http://api.test".to_string()),
            ]
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_step_with_nowhere_to_go_stops_the_file_before_anything_is_dialled() {
        let content = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- ENDPOINT ---\nGET /v1/orders\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parser::parse_gctf_from_str(content, "checkout.apif").unwrap();
        let runner = TestRunner::new(false, 30, false, false, false, None);

        let (result, _) = runner
            .run_chain(&doc, HashMap::new())
            .await
            .expect("returns");

        match result.status {
            TestExecutionStatus::Fail(said) => {
                assert!(said.contains("no address for /v1/orders"), "{said}");
                assert!(said.contains("nothing was dialled"), "{said}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(
            result.document_durations_ms.is_empty(),
            "no step ran, so none has a duration",
        );
    }

    #[test]
    fn a_transport_nobody_addressed_has_nowhere_to_go() {
        let content = "--- ADDRESS ---\n127.0.0.1:4770\n\n--- ENDPOINT ---\nauth.v1.Auth/Login\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n\n--- ENDPOINT ---\nGET /v1/orders\n\n--- ASSERTS ---\n@status() == 200\n";
        let doc = crate::parser::parse_gctf_from_str(content, "checkout.apif").unwrap();
        assert_eq!(
            chain_addresses(&doc),
            vec![Some("127.0.0.1:4770".to_string()), None]
        );
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn run_test_capturing_vars_returns_result_and_map() {
        let content = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- REQUEST ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(content, "capture.gctf").unwrap();
        let runner = TestRunner::new(true, 30, false, false, false, None);

        let (result, vars) = runner.run_test_capturing_vars(&doc).await.unwrap();
        assert!(matches!(result.status, TestExecutionStatus::Pass));
        assert!(vars.is_empty());
    }
    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_whole_url_in_the_endpoint_is_its_own_address() {
        let runner = TestRunner::new(true, 30, false, false, false, None);
        let aimed = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\nGET https://api.example.com/v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        )
        .expect("parses");
        let steps: Vec<&crate::parser::GctfDocument> = aimed.iter_chain().collect();
        assert!(runner.step_with_nowhere_to_go(&steps, &[None]).is_none());

        let bare = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
            "t.httf",
        )
        .expect("parses");
        let steps: Vec<&crate::parser::GctfDocument> = bare.iter_chain().collect();
        assert!(runner.step_with_nowhere_to_go(&steps, &[None]).is_some());
    }

    #[test]
    fn a_plan_says_which_sections_a_run_walks_past() {
        let doc = crate::parser::parse_gctf_from_str(
            "--- ENDPOINT ---\npkg.Svc/M\n\n#[skip]\n--- REQUEST ---\n{\"a\": 1}\n\n--- REQUEST ---\n{\"a\": 2}\n\n#[skip]\n--- RESPONSE ---\n{}\n\n--- ASSERTS ---\n.a == 1\n",
            "t.gctf",
        )
        .expect("parses");
        let plan = ExecutionPlan::from_document(&doc);
        assert_eq!(
            plan.requests.iter().map(|r| r.skipped).collect::<Vec<_>>(),
            vec![true, false],
        );
        assert!(plan.expectations.iter().all(|e| e.skipped));
        assert!(plan.assertions.iter().all(|a| !a.skipped));
        assert_eq!(plan.summary.skipped_sections, 2);
        assert_eq!(
            plan.summary.total_requests, 2,
            "the totals stay what the file holds"
        );
    }
}
