use super::super::parser::GctfDocument;
use super::runner_helpers;
use super::{AssertionHandler, RequestHandler, RequestSendResult, ResponseHandler};
use crate::assert::{AssertionEngine, JsonComparator, get_json_diff};
use crate::grpc::{GrpcClient, GrpcClientConfig};
use crate::optimizer;
use crate::parser::ast::{SectionContent, SectionType};
use crate::plugins::AssertionTiming;
use crate::report::CoverageCollector;
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

/// Global plugin registry used by all assertion engines.
static PLUGIN_REGISTRY: LazyLock<Arc<dyn apif_assert::registry::PluginRegistry>> =
    LazyLock::new(|| Arc::new(crate::execution::plugin_dir::build_plugin_manager()));

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
    pub response_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionInfo {
    pub index: usize,
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

/// Actual RPC mode from proto descriptor (for runtime validation)
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
    pub rpc_mode_name: String,
}

struct AssertionContext<'a> {
    headers: &'a HashMap<String, String>,
    trailers: &'a HashMap<String, String>,
    timing: Option<&'a AssertionTiming>,
    /// EXTRACT-bound variables, so `$name` references in ASSERTS resolve.
    variables: &'a HashMap<String, Value>,
    /// Wire protocol that produced this response (`"grpc"`/`"grpc-web"`/
    /// `"connectrpc"`) — forwarded to assertion plugins via `PluginContext`.
    protocol: &'static str,
}

impl ExecutionPlan {
    /// Build execution plan from a GctfDocument
    pub fn from_document(doc: &GctfDocument) -> Self {
        let file_path = doc.file_path.clone();

        let connection = if let Some(section) = doc.first_section(SectionType::Address) {
            if let SectionContent::Single(addr) = &section.content {
                ConnectionInfo {
                    address: addr.clone(),
                    source: format!(
                        "ADDRESS section [Line {}-{}]",
                        section.start_line, section.end_line
                    ),
                    backend: "default".to_string(),
                }
            } else {
                ConnectionInfo {
                    address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                    source: "Environment variable (implicit)".to_string(),
                    backend: "default".to_string(),
                }
            }
        } else {
            ConnectionInfo {
                address: "<env:GRPCTESTIFY_ADDRESS>".to_string(),
                source: "Environment variable (implicit)".to_string(),
                backend: "default".to_string(),
            }
        };

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
                    lines
                        .iter()
                        .map(|line| {
                            optimizer::rewrite_assertion_expression_fixed_point_if_changed_with_level(line, optimizer::OptimizeLevel::Safe)
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

/// Get actual RPC mode from method descriptor
/// What `run_one` derives from the document rather than from the request.
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

/// One entry per chain document. `None` where deriving it would swallow an
/// error the ordinary path reports.
pub struct PreparedChain(Vec<Option<PreparedDocument>>);

/// Dropping a `JoinHandle` only detaches the task; nothing here wants an
/// abandoned in-flight call per request.
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

/// Format protocol name for display in verbose output.
fn protocol_display(protocol: crate::grpc::WireProtocol) -> &'static str {
    match protocol {
        crate::grpc::WireProtocol::Grpc => "gRPC",
        crate::grpc::WireProtocol::GrpcWeb => "gRPC-Web",
        crate::grpc::WireProtocol::ConnectRpc => "ConnectRPC",
    }
}

/// Canonical machine-readable protocol string — the same form
/// `OPTIONS.protocol:` accepts (`WireProtocol::from_str`) — forwarded to
/// assertion plugins via `PluginContext::protocol`, distinct from
/// [`protocol_display`]'s human-facing form.
fn protocol_str(protocol: crate::grpc::WireProtocol) -> &'static str {
    match protocol {
        crate::grpc::WireProtocol::Grpc => "grpc",
        crate::grpc::WireProtocol::GrpcWeb => "grpc-web",
        crate::grpc::WireProtocol::ConnectRpc => "connectrpc",
    }
}

/// Infer RPC mode from GCTF section structure (without proto descriptor)
pub(crate) fn infer_rpc_mode_for_section_types(document: &GctfDocument) -> RpcModeInfo {
    // A single JsonLines REQUEST section sends N messages on one stream, same
    // as N separate REQUEST sections — count messages, not sections, so
    // client/bidi-streaming inference works either way.
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

/// Check compatibility between inferred and actual RPC mode
fn check_rpc_mode_compatibility(inferred: &RpcModeInfo, actual: &RpcModeInfo) -> Option<String> {
    match (inferred, actual) {
        // Compatible pairs
        (RpcModeInfo::Unary, RpcModeInfo::Unary) => None,
        (RpcModeInfo::ServerStreaming, RpcModeInfo::ServerStreaming) => None,
        (RpcModeInfo::ClientStreaming, RpcModeInfo::ClientStreaming) => None,
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::BidirectionalStreaming) => None,
        (RpcModeInfo::Unknown, _) => None, // Can't validate unknown
        (_, RpcModeInfo::Unknown) => None,  // Actual unknown means no descriptor

        // Incompatible: inferred Unary but actual is streaming
        (RpcModeInfo::Unary, RpcModeInfo::ServerStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Server Streaming. Client will send ONE request and expect multiple responses.".to_string())
        }
        (RpcModeInfo::Unary, RpcModeInfo::ClientStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Client Streaming. Client will send ONE request but server expects multiple.".to_string())
        }
        (RpcModeInfo::Unary, RpcModeInfo::BidirectionalStreaming) => {
            Some("gCTF defines Unary RPC but proto expects Bidirectional Streaming. Client will send ONE request but server expects stream.".to_string())
        }

        // Incompatible: inferred streaming but actual is Unary
        (RpcModeInfo::ServerStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Server Streaming (multiple RESPONSE sections) but proto expects Unary. gCTF may fail.".to_string())
        }
        (RpcModeInfo::ClientStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Client Streaming (multiple REQUEST sections) but proto expects Unary. gCTF may fail.".to_string())
        }
        (RpcModeInfo::BidirectionalStreaming, RpcModeInfo::Unary) => {
            Some("gCTF defines Bidirectional Streaming but proto expects Unary. gCTF may fail.".to_string())
        }

        // Cross-streaming mismatches
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

/// Counts messages, not sections: one json-lines REQUEST/RESPONSE carries N
/// messages on a single stream, which is the same shape as N separate sections.
/// Counting sections made this disagree with `Workflow::rpc_mode_name` (which
/// counts events) for the same file, so `inspect`'s text and JSON output gave
/// two different modes.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExecutionStatus {
    Pass,
    Fail(String),
}

/// Classifies *why* a test failed, so callers (e.g. retry logic) can key off the
/// real error kind instead of pattern-matching the failure message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Connection/stream/transport-level failure. Potentially retryable
    /// (subject to the gRPC status code).
    Transport,
    /// Assertion mismatch, validation, parse, or configuration error.
    /// Never retryable.
    Assertion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestExecutionResult {
    pub status: TestExecutionStatus,
    pub call_duration_ms: Option<u64>,
    /// The same span as `call_duration_ms` at nanosecond resolution: the gRPC
    /// call plus response validation, measured from after the client and
    /// descriptors are resolved. Milliseconds are useless for the sub-millisecond
    /// RPCs a load run measures, and `bench` needs this rather than wall-clock
    /// around the whole document — that would fold client construction, `Value`
    /// clones and variable substitution into the server's reported latency.
    pub call_duration_ns: Option<u64>,
    pub captured_response: Option<crate::grpc::GrpcResponse>,
    pub meta: crate::state::TestMeta,
    /// What this test declared (sections, TLS, DATASET rows, PROTO files) —
    /// see [`apif_state::ConfigSummary`].
    pub config_summary: apif_state::ConfigSummary,
    /// Set only when `status` is `Fail`; classifies the failure for retry logic.
    pub failure_kind: Option<FailureKind>,
    /// Numeric gRPC status code observed for the call, when one is available.
    /// `Some(0)` for an OK response, `Some(code)` when the server/transport
    /// returned a gRPC status, and `None` when the request never produced a
    /// gRPC status (e.g. a pure assertion/config failure).
    pub grpc_status: Option<u32>,
    /// Per-assertion outcome + timing, in source order. Empty for pre-flight
    /// failures (e.g. bad OPTIONS) that never reach assertion evaluation.
    pub assertions: Vec<apif_state::AssertionRecord>,
    /// `true` when at least one REQUEST needed more than one attempt to
    /// succeed — a Pass with `retried = true` is flaky, not a clean pass.
    pub retried: bool,
    /// Real wall-clock duration of each document in the chain, in source
    /// order — only populated on the whole-chain result [`ChainAccumulator`]
    /// produces (per-document results have their own duration in
    /// `call_duration_ms` instead). Lets reporters give each document its
    /// actual timing instead of splitting the total evenly.
    pub document_durations_ms: Vec<u64>,
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
            document_durations_ms: Vec::new(),
        }
    }

    /// Attach the nanosecond-resolution call duration.
    pub fn with_call_duration_ns(mut self, ns: u64) -> Self {
        self.call_duration_ns = Some(ns);
        self
    }

    /// Build a failure. Defaults to `Assertion` (non-retryable); transport-level
    /// failures should override via [`with_failure_kind`].
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

/// Folds a multi-document chain's per-document [`TestExecutionResult`]s into
/// one aggregate. Shared by `run_test_with_variables` and
/// `run_test_capturing_vars`, which differ only in whether they also return
/// the final variable map.
#[derive(Default)]
struct ChainAccumulator {
    status: Option<TestExecutionStatus>,
    failure_kind: Option<FailureKind>,
    grpc_status: Option<u32>,
    total_duration_ms: f64,
    total_duration_ns: u64,
    assertions: Vec<apif_state::AssertionRecord>,
    /// Last document's captured response wins — write mode/exchange capture
    /// always target a single physical file, so only the final call's
    /// response (the one that matters for snapshotting) is kept.
    captured_response: Option<crate::grpc::GrpcResponse>,
    /// `true` once any document in the chain retried — sticky across the
    /// whole chain, since one flaky request makes the overall test flaky.
    retried: bool,
    /// Each absorbed document's own wall-clock duration, in chain order —
    /// including the failing document's, when the chain stops early.
    document_durations_ms: Vec<u64>,
}

impl ChainAccumulator {
    /// Fold one document's result in. Returns `true` when the chain should
    /// stop (this document failed — fail-fast).
    fn absorb(&mut self, mut result: TestExecutionResult) -> bool {
        if let Some(dur) = result.call_duration_ms {
            self.total_duration_ms += dur as f64;
        }
        self.total_duration_ns += result.call_duration_ns.unwrap_or(0);
        self.document_durations_ms
            .push(result.call_duration_ms.unwrap_or(0));
        self.grpc_status = result.grpc_status;
        self.retried |= result.retried;
        self.assertions.append(&mut result.assertions);
        if result.captured_response.is_some() {
            self.captured_response = result.captured_response.take();
        }

        if matches!(result.status, TestExecutionStatus::Fail(_)) {
            self.failure_kind = result.failure_kind;
            self.status = Some(result.status);
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
            document_durations_ms: self.document_durations_ms,
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
    /// Client-side connection pool slot. Threaded into `GrpcClientConfig` so the
    /// transport channel cache keys by it — distinct ids open distinct channels.
    connection_id: u64,
    address_override: Option<String>,
    /// When true, capture headers/trailers/response messages even outside
    /// write mode, so reports (e.g. Allure attachments) can show what actually
    /// happened. Off by default: skips the extra buffering unless requested.
    capture_exchange: bool,
    assertion_engine: AssertionEngine,
    coverage_collector: Option<Arc<CoverageCollector>>,
    request_handler: RequestHandler,
    response_handler: ResponseHandler,
    assertion_handler: AssertionHandler,
}

impl TestRunner {
    /// Create expected values from a response section.
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
                section.start_line
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
            capture_exchange: false,
            assertion_engine: AssertionEngine::with_registry(PLUGIN_REGISTRY.clone()),
            coverage_collector: coverage_collector.clone(),
            request_handler: RequestHandler::new(no_assert, verbose, coverage_collector.clone()),
            response_handler: ResponseHandler::new(no_assert),
            assertion_handler: AssertionHandler::new(verbose),
        }
    }

    /// Set protocol override. When set, this takes priority over the GCTF file's OPTIONS.protocol.
    pub fn with_protocol(mut self, protocol: crate::grpc::WireProtocol) -> Self {
        self.protocol_override = Some(protocol);
        self
    }

    /// Send every request here regardless of the document's ADDRESS section.
    pub fn with_address_override(mut self, address: String) -> Self {
        self.address_override = Some(address);
        self
    }

    /// Ask the runner to capture headers/trailers/response messages for every
    /// test, not just in write mode, so reports can show what actually
    /// happened. Capped internally — see [`apif_state::CapturedExchange`].
    pub fn with_capture_exchange(mut self, capture: bool) -> Self {
        self.capture_exchange = capture;
        self
    }

    /// Assign the connection-pool slot for this runner. Distinct ids map to
    /// distinct cached transport channels (see `GrpcClientConfig::connection_id`).
    pub fn with_connection_id(mut self, connection_id: u64) -> Self {
        self.connection_id = connection_id;
        self
    }

    /// Whether snapshot-update mode (`--write`) is on. Callers must consult this
    /// before rewriting a `.gctf` from a captured response: `capture_exchange`
    /// also populates `captured_response`, and it is enabled by report formats
    /// that have nothing to do with `--write`.
    pub fn is_write_mode(&self) -> bool {
        self.write_mode
    }

    /// Run a test document chain.
    /// Walks the `next_document` linked list, accumulating EXTRACT variables
    /// between documents. Fail-fast: stops on first failure.
    pub async fn run_test(&self, document: &GctfDocument) -> Result<TestExecutionResult> {
        self.run_test_with_variables(document, HashMap::new()).await
    }

    /// Run a test document chain with pre-populated variables (for data-driven bench).
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

    /// Derive what is constant for this document once, for callers that issue
    /// many requests from it.
    pub fn prepare(&self, document: &GctfDocument) -> PreparedChain {
        PreparedChain(
            document
                .iter_chain()
                .map(|doc| {
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
                            None => runner_helpers::effective_address(doc, self.protocol_override),
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

    /// [`run_test_with_variables`] with the derivation done by the caller.
    pub async fn run_test_prepared(
        &self,
        document: &GctfDocument,
        initial_variables: HashMap<String, Value>,
        prepared: &PreparedChain,
    ) -> Result<TestExecutionResult> {
        let mut variables = initial_variables;
        let mut acc = ChainAccumulator::default();

        for (doc, prep) in document.iter_chain().zip(prepared.0.iter()) {
            let result = self.run_one(doc, &mut variables, prep.as_ref()).await?;
            if acc.absorb(result) {
                break;
            }
        }

        Ok(acc.into_result())
    }

    pub async fn run_test_with_variables(
        &self,
        document: &GctfDocument,
        initial_variables: HashMap<String, Value>,
    ) -> Result<TestExecutionResult> {
        let mut variables = initial_variables;
        let mut acc = ChainAccumulator::default();

        for doc in document.iter_chain() {
            let result = self.run_one(doc, &mut variables, None).await?;
            if acc.absorb(result) {
                break;
            }
        }

        Ok(acc.into_result())
    }

    /// Run a test document chain like [`run_test_with_variables`], but also
    /// return the final accumulated variable map (post-EXTRACT). Used to seed
    /// per-directory fixtures: a `_setup.gctf`'s EXTRACT bindings become the
    /// initial variables for the tests in that directory. Execution semantics
    /// are identical to a normal chain run started with no initial variables.
    pub async fn run_test_capturing_vars(
        &self,
        document: &GctfDocument,
    ) -> Result<(TestExecutionResult, HashMap<String, Value>)> {
        let mut variables: HashMap<String, Value> = HashMap::new();
        let mut acc = ChainAccumulator::default();

        for doc in document.iter_chain() {
            let result = self.run_one(doc, &mut variables, None).await?;
            if acc.absorb(result) {
                break;
            }
        }

        Ok((acc.into_result(), variables))
    }

    /// Run a single document, sharing variables with the chain.
    async fn run_one(
        &self,
        document: &GctfDocument,
        variables: &mut HashMap<String, Value>,
        prepared: Option<&PreparedDocument>,
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

        // Canonical precedence: section attribute > OPTIONS > env default.
        // An explicit-but-invalid value is a configuration error, not a fall-back.
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
                None => runner_helpers::effective_address(document, self.protocol_override),
            },
        };

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

        // Substitute variables in request header values, then guard against any
        // undefined/typo'd placeholder being shipped verbatim in metadata.
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

        // Field coverage is the only consumer; without a collector this is two
        // descriptor-pool walks and two allocations per request for nothing.
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

        // Phase 1: RPC mode. The section shape is only a guess -- a
        // server-streaming method with one RESPONSE looks unary, and an ERROR
        // section hides streaming entirely. gRPC does not care, but Connect and
        // gRPC-Web pick the content type and the framing from the mode, and a
        // streaming method addressed as unary is rejected with 415. So on those
        // transports the descriptor wins whenever it knows the method.
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

        // Determine RPC mode for HTTP transport (connectrpc/grpc-web needs to know streaming type)
        let rpc_mode: Option<crate::grpc::RpcMode> = match wire_rpc_mode {
            RpcModeInfo::Unary => Some(crate::grpc::RpcMode::Unary),
            RpcModeInfo::ServerStreaming => Some(crate::grpc::RpcMode::ServerStream),
            RpcModeInfo::ClientStreaming => Some(crate::grpc::RpcMode::ClientStream),
            RpcModeInfo::BidirectionalStreaming => Some(crate::grpc::RpcMode::Bidi),
            RpcModeInfo::Unknown => None,
        };

        // For HTTP bidi, the runner must send all requests before reading any responses.
        let is_http_bidi = client_protocol != crate::grpc::WireProtocol::Grpc
            && rpc_mode == Some(crate::grpc::RpcMode::Bidi);
        let mut deferred_bidi_expectations: Vec<Value> = Vec::new();

        // Start the gRPC call in background so unary/server-streaming methods can wait
        // for the first request message without deadlocking this task.
        let full_service_clone = full_service.clone();
        let method_clone = method.clone();
        let mut client_for_call = client;
        let mut call_handle = Some(AbortOnDrop(Some(tokio::spawn(Box::pin(async move {
            client_for_call
                .call_stream(&full_service_clone, &method_clone, request_stream, rpc_mode)
                .await
        })))));

        let mut response_stream = None;

        // variables passed from caller (shared across chain)
        // When the wire went quiet. Everything after it — assertion
        // evaluation, JSON diffing, snapshot capture — is our cost, not the
        // server's, and must not land in the reported latency.
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
        // Transport-level failures (connection refused, stream startup errors,
        // stream read timeouts, ...) must never be masked in write mode and
        // must never trigger a snapshot rewrite.
        let mut transport_failure = false;
        // Set when any REQUEST's retry loop needed more than one attempt to
        // succeed — surfaced on the result so a test that only passed after a
        // retry can be flagged flaky instead of looking like a clean pass.
        let mut retry_occurred = false;
        // Numeric gRPC status observed on this call (from a returned GrpcError),
        // surfaced on the result so bench can bucket by real status code.
        let mut grpc_status: Option<u32> = None;

        // We iterate by index to allow lookahead
        let sections = &document.sections;

        // Pre-compute last request index to avoid O(n²) lookups in loop
        let last_request_idx = sections
            .iter()
            .rposition(|s| s.section_type == SectionType::Request);

        let has_request_sections = sections
            .iter()
            .any(|s| s.section_type == SectionType::Request);

        // Legacy behavior: if no REQUEST section is provided, send an empty
        // JSON object as a single request message for unary/server-stream calls.
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

        /// Awaits the gRPC call handle and extracts the response stream.
        /// Eliminates duplication across Response and Asserts section handlers.
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
                        section.start_line,
                        repeat_iter + 1,
                        repeat_count
                    );
                }

                match section.section_type {
                    SectionType::Request => {
                        // A JsonLines REQUEST sends one message per value, in
                        // order, on the same stream — the client/bidi-streaming
                        // symmetric counterpart to RESPONSE's JsonLines (one
                        // expected value per streamed message received).
                        let request_values: Vec<Value> = match &section.content {
                            SectionContent::Json(req_json) => vec![req_json.clone()],
                            SectionContent::JsonLines(values) => values.clone(),
                            SectionContent::Empty => vec![Value::Object(serde_json::Map::new())],
                            _ => continue,
                        };

                        for mut request_value in request_values {
                            // The parser proves at load time whether the file
                            // holds a `{{` anywhere; when it does not, both
                            // walks below can only report "nothing changed".
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
                                    // An undefined/typo'd variable must never be
                                    // shipped to the wire as a literal `{{...}}`.
                                    return Ok(TestExecutionResult::fail(
                                        format!(
                                            "Unresolved variable placeholder(s) in REQUEST at line {}: {}",
                                            section.start_line,
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
                                    section.start_line
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
                                        section.start_line,
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
                                        .send_request(tx_ref, payload, section.start_line, None)
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
                                    section.start_line,
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

                        // For HTTP bidi, buffer expected values until stream is ready.
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
                        // Merge deferred expectations (from earlier skipped response sections)
                        // with the current section's expectations in order.
                        let expected_values: Vec<Value> = deferred_bidi_expectations
                            .drain(..)
                            .chain(section_expected)
                            .collect();

                        let Some(stream) = response_stream.as_mut() else {
                            failure_reasons.push(format!(
                                "No response stream available for RESPONSE section at line {}",
                                section.start_line
                            ));
                            transport_failure = true;
                            break;
                        };

                        let read_timeout_secs = get_timeout().unwrap_or(effective_timeout_seconds);

                        // A RESPONSE with no messages is the only way to assert
                        // that a server-streaming call yielded none. It used to
                        // skip the loop below and pass whatever arrived.
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
                                    section.start_line,
                                    msg
                                ));
                            }
                        }

                        let mut stream_read_timed_out = false;
                        for expected_template in expected_values {
                            // Bound each stream read: tonic's Endpoint::timeout only
                            // covers the request/headers phase, not stalled streams.
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
                                            read_timeout_secs, section.start_line
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

                                            if !diffs.is_empty() {
                                                self.append_response_diffs(
                                                    diffs,
                                                    section.start_line,
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
                                                section.start_line
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
                                        section.start_line,
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
                                        section.start_line
                                    ));
                                    }
                                    break;
                                }
                            }
                        }

                        if stream_read_timed_out {
                            // The stream is stalled; drop it so later sections
                            // fail fast instead of timing out one by one.
                            response_stream = None;
                        }

                        // If this RESPONSE is the last section that reads from the
                        // stream, drain any trailing items so trailers are captured
                        // before the attached assertions run. Otherwise `@trailer(...)`
                        // on the final RESPONSE sees empty trailers, because trailers
                        // arrive only after the last message. We do NOT drain when a
                        // later section still needs the stream (avoids stealing its
                        // messages / reordering multi-response streams).
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
                                            // Expectations already satisfied; a stalled
                                            // trailer read must not fail the test.
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
                                            // Over-delivery beyond expectations: keep for
                                            // snapshot fidelity but do not assert on it.
                                            if let Some(resp) = &mut captured_response {
                                                resp.messages.push(msg);
                                            }
                                        }
                                        // Error or end-of-stream: nothing more to capture.
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
                                            section.start_line
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
                            section.start_line
                        ));
                        }
                    }
                    SectionType::Asserts => {
                        if i >= last_request_idx.unwrap_or(usize::MAX) {
                            drop(tx.take());
                        }

                        ensure_stream_ready!();

                        // If we have a captured error context (from a preceding ERROR section),
                        // use that instead of reading from the stream.
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
                                        format!("after ERROR at line {}", section.start_line),
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
                                        format!("after ERROR at line {}", section.start_line),
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
                                    section.start_line
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
                                        read_timeout_secs, section.start_line
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
                                        format!("at line {}", section.start_line),
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
                                // Trailers arrive at end-of-stream. A standalone ASSERTS
                                // positioned here is asserting on trailers/status rather
                                // than a message body: evaluate it against the last
                                // received message (or Null when none) with the captured
                                // trailers in context, so `@trailer(...)`/`@has_trailer(...)`
                                // work instead of the section unconditionally failing.
                                if !effective_no_assert
                                    && let SectionContent::Assertions(lines) = &section.content
                                {
                                    let target = last_message.clone().unwrap_or(Value::Null);
                                    self.run_assertions(
                                        lines,
                                        &target,
                                        &mut failure_reasons,
                                        &mut assertion_records,
                                        format!("(trailers) at line {}", section.start_line),
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
                                        format!("after ERROR at line {}", section.start_line),
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
                                                tracing::trace!(
                                                    "extract: {key} = {query} -> {val:?}"
                                                );
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
                                            // Code 14 = UNAVAILABLE: connection-level
                                            // failure, not a genuine server response —
                                            // never snapshot it.
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

                                            // Try to extract tonic Status from anyhow::Error
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
                                                // Fallback to error string representation
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
                                                    section.start_line
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
                                                        section.start_line
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

                                    // Error has been consumed at startup stage; continue with next sections.
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
                                section.start_line
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
                                        read_timeout_secs, section.start_line
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
                                    // Genuine error response received from an
                                    // established stream — capture for snapshots.
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
                                            section.start_line
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
                                                section.start_line
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
                                        section.start_line
                                    ));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            } // close repeat_iter
        } // close for (i, section)

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

        // Tag this document's assertions with its endpoint so multi-document
        // chains group per endpoint in verbose reports.
        if assertion_records.iter().any(|rec| rec.endpoint.is_none()) {
            let endpoint_label = format!("{}/{}", full_service, method);
            for rec in &mut assertion_records {
                if rec.endpoint.is_none() {
                    rec.endpoint = Some(endpoint_label.clone());
                }
            }
        }

        // Ends when the wire did, not when validation finished. Falls back to
        // "now" only when nothing was ever received (a setup failure), where
        // the two are the same anyway.
        let grpc_elapsed = rpc_end.map_or_else(
            || start_time.elapsed(),
            |end| end.saturating_duration_since(start_time),
        );
        let grpc_duration = grpc_elapsed.as_millis() as u64;
        let grpc_duration_ns = grpc_elapsed.as_nanos() as u64;

        if !failure_reasons.is_empty() {
            // In write mode, assertion mismatches are expected because we are
            // updating snapshots — but transport failures (connection refused,
            // stream startup errors, timeouts) must stay failures and must not
            // rewrite the file with an empty/partial response.
            if effective_write_mode
                && !transport_failure
                && let Some(resp) = captured_response
            {
                return Ok(TestExecutionResult::pass(Some(grpc_duration))
                    .with_call_duration_ns(grpc_duration_ns)
                    .with_response(resp)
                    .with_grpc_status(grpc_status.unwrap_or(0))
                    .with_assertions(assertion_records)
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
            .with_retried(retry_occurred);
            // Only surface a gRPC status for transport failures (the real code
            // the server/transport returned). A pure assertion failure reached
            // no defined gRPC status, so it stays `None`.
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
            .with_retried(retry_occurred);
        if !transport_failure && let Some(resp) = captured_response {
            result = result.with_response(resp);
        }
        Ok(result)
    }

    /// Validates a collected response against the document (for testing purposes)
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
        let mut optimized_lines: Option<Vec<String>> = None;

        for (idx, line) in lines.iter().enumerate() {
            if let Some(rewritten) =
                optimizer::rewrite_assertion_expression_fixed_point_if_changed_with_level(
                    line,
                    optimizer::OptimizeLevel::Safe,
                )
            {
                let vec = optimized_lines.get_or_insert_with(|| lines[..idx].to_vec());
                vec.push(rewritten);
            } else if let Some(vec) = optimized_lines.as_mut() {
                vec.push(line.clone());
            }
        }

        let lines_to_evaluate: &[String] = optimized_lines.as_deref().unwrap_or(lines);

        let result = self.assertion_handler.evaluate_assertions_for_section(
            lines_to_evaluate,
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

    /// Format JSON comparison diffs and append to failure_reasons.
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

    /// Log a response message for debug/verbose/raw modes.
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

    fn runner() -> TestRunner {
        TestRunner::new(false, 30, false, false, false, None)
    }

    fn parse(text: &str) -> crate::parser::GctfDocument {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.gctf");
        std::fs::write(&path, text).expect("write");
        crate::parser::parse_with_recovery(&path).document
    }

    // Preparation must reproduce what the ordinary path derives, or a bench run
    // silently talks to somewhere else.
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

    // A document whose OPTIONS cannot be honoured must fall back, so the error
    // is still reported instead of being papered over with a default.
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

    // Dropping a `JoinHandle` only detaches the task. Under load that left one
    // abandoned gRPC call per request in the runtime.
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

    // Regression: `run_test_with_variables`/`run_test_capturing_vars` used to
    // hardcode `captured_response: None` on the aggregate, discarding every
    // per-document response — so `--write` snapshot mode never rewrote files
    // with the real captured response for the happy path. `ChainAccumulator`
    // carries the last document's response through.
    // A single JsonLines REQUEST section sending 3 messages must infer the
    // same ClientStreaming mode as 3 separate REQUEST sections would — the
    // inference must count messages, not sections, or `rpc_mode` (which
    // drives the actual wire streaming type for non-gRPC protocols) silently
    // misclassifies a JsonLines request as unary.
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
        };
        acc.absorb(TestExecutionResult::pass(None).with_assertions(vec![record.clone()]));
        acc.absorb(TestExecutionResult::pass(None).with_assertions(vec![record.clone()]));

        assert_eq!(acc.into_result().assertions.len(), 2);
    }

    // Regression: `retried` must be sticky across a chain — one flaky document
    // makes the whole chain flaky, even if later documents didn't need a retry.
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

    // Regression: each document's own duration must be collected in order, not
    // just summed — reporters need the individual values for real step timing.
    #[test]
    fn chain_accumulator_collects_per_document_durations_in_order() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(Some(50)));
        acc.absorb(TestExecutionResult::pass(Some(30)));
        let result = acc.into_result();
        assert_eq!(result.document_durations_ms, vec![50, 30]);
        // total_duration_ms (call_duration_ms) still sums, unaffected.
        assert_eq!(result.call_duration_ms, Some(80));
    }

    #[test]
    fn chain_accumulator_missing_duration_recorded_as_zero() {
        let mut acc = ChainAccumulator::default();
        acc.absorb(TestExecutionResult::pass(None));
        assert_eq!(acc.into_result().document_durations_ms, vec![0]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn runner_new() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        assert!(!runner.dry_run);
        assert_eq!(runner.timeout_seconds, 30);
        assert!(!runner.no_assert);
        assert!(!runner.write_mode);
        assert!(!runner.verbose);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn runner_with_dry_run() {
        let runner = TestRunner::new(true, 30, false, false, false, None);
        assert!(runner.dry_run);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn runner_with_timeout() {
        let runner = TestRunner::new(false, 60, false, false, false, None);
        assert_eq!(runner.timeout_seconds, 60);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn runner_with_no_assert() {
        let runner = TestRunner::new(false, 30, true, false, false, None);
        assert!(runner.no_assert);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
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
    #[cfg_attr(miri, ignore)]
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
    #[cfg_attr(miri, ignore)]
    fn substitute_variables_exact_match_preserves_type() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("{{ count }}");
        let mut vars = HashMap::new();
        vars.insert("count".to_string(), json!(42));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!(42));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
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
    #[cfg_attr(miri, ignore)]
    fn substitute_variables_keeps_unknown_placeholder() {
        let runner = TestRunner::new(false, 30, false, false, false, None);
        let mut value = json!("hello {{known}} and {{unknown}}");
        let mut vars = HashMap::new();
        vars.insert("known".to_string(), json!("world"));

        runner.substitute_variables(&mut value, &vars);
        assert_eq!(value, json!("hello world and {{unknown}}"));
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

        // The function returns values for any Json content, not just Response sections
        // This is expected behavior - it extracts Json values regardless of section type
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
        // Returns 1 because the content is Json, even though it's a Request section
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
        assert!(failures[0].contains("ERROR at line 12 has 'with_asserts'"));
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

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn run_test_capturing_vars_returns_result_and_map() {
        // Dry-run short-circuits before any network call, so this exercises the
        // additive method's plumbing (result + variable map) without a server.
        let content = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- REQUEST ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(content, "capture.gctf").unwrap();
        let runner = TestRunner::new(true, 30, false, false, false, None);

        let (result, vars) = runner.run_test_capturing_vars(&doc).await.unwrap();
        assert!(matches!(result.status, TestExecutionStatus::Pass));
        // No EXTRACT was executed (dry-run), so the captured map is empty, but a
        // map is returned alongside the result as the fixture seeding relies on.
        assert!(vars.is_empty());
    }
}
