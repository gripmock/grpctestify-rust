use apif_ast::{GctfDocument, SectionContent, SectionType};
use apif_optimizer::{self, OptimizeLevel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
    pub assertions: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub response_index: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionInfo {
    pub index: usize,
    pub variables: HashMap<String, String>,
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
    pub rpc_mode_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExecutionStatus {
    Pass,
    Fail(String),
}
#[derive(Debug, Clone, PartialEq)]
pub struct TestExecutionResult {
    pub status: TestExecutionStatus,
    pub call_duration_ms: Option<u64>,
    pub captured_response: Option<apif_grpc_transport::types::GrpcResponse>,
    pub meta: apif_state::TestMeta,
}
impl TestExecutionResult {
    pub fn pass(call_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Pass,
            call_duration_ms,
            captured_response: None,
            meta: apif_state::TestMeta::default(),
        }
    }
    pub fn fail(message: String, call_duration_ms: Option<u64>) -> Self {
        Self {
            status: TestExecutionStatus::Fail(message),
            call_duration_ms,
            captured_response: None,
            meta: apif_state::TestMeta::default(),
        }
    }
    pub fn with_response(mut self, r: apif_grpc_transport::types::GrpcResponse) -> Self {
        self.captured_response = Some(r);
        self
    }
    pub fn with_meta(mut self, m: apif_state::TestMeta) -> Self {
        self.meta = m;
        self
    }
}

impl ExecutionPlan {
    pub fn from_document(doc: &GctfDocument) -> Self {
        let conn = doc
            .first_section(SectionType::Address)
            .map(|s| match &s.content {
                SectionContent::Single(a) => ConnectionInfo {
                    address: a.clone(),
                    source: format!("ADDRESS [L{}-{}]", s.start_line, s.end_line),
                    backend: "default".into(),
                },
                _ => ConnectionInfo {
                    address: "<env:GRPCTESTIFY_ADDRESS>".into(),
                    source: "Environment".into(),
                    backend: "default".into(),
                },
            })
            .unwrap_or_else(|| ConnectionInfo {
                address: "<env:GRPCTESTIFY_ADDRESS>".into(),
                source: "Environment".into(),
                backend: "default".into(),
            });

        let target = doc
            .first_section(SectionType::Endpoint)
            .map(|s| match &s.content {
                SectionContent::Single(e) => {
                    let (p, sv, m) = doc
                        .parse_endpoint()
                        .map(|(p, s, m)| (Some(p), Some(s), Some(m)))
                        .unwrap_or((None, None, None));
                    TargetInfo {
                        endpoint: e.clone(),
                        package: p,
                        service: sv,
                        method: m,
                    }
                }
                _ => TargetInfo {
                    endpoint: "<missing>".into(),
                    package: None,
                    service: None,
                    method: None,
                },
            })
            .unwrap_or_else(|| TargetInfo {
                endpoint: "<missing>".into(),
                package: None,
                service: None,
                method: None,
            });

        let headers = doc
            .first_section(SectionType::RequestHeaders)
            .and_then(|s| match &s.content {
                SectionContent::KeyValues(h) => Some(HeadersInfo {
                    count: h.len(),
                    headers: h.clone(),
                }),
                _ => None,
            });

        let requests: Vec<RequestInfo> = doc
            .sections_by_type(SectionType::Request)
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (c, ct) = match &s.content {
                    SectionContent::Json(j) => (j.clone(), "json"),
                    SectionContent::JsonLines(_) => (Value::Array(vec![]), "json-lines"),
                    SectionContent::Empty => (Value::Object(serde_json::Map::new()), "empty"),
                    _ => (Value::Null, "unknown"),
                };
                RequestInfo {
                    index: i + 1,
                    content: c,
                    content_type: ct.into(),
                    line_start: s.start_line,
                    line_end: s.end_line,
                }
            })
            .collect();

        let resp_sects = doc.sections_by_type(SectionType::Response);
        let err_sect = doc.first_section(SectionType::Error);
        let expectations = if !resp_sects.is_empty() {
            resp_sects
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (c, mc) = match &s.content {
                        SectionContent::Json(j) => (Some(j.clone()), None),
                        SectionContent::JsonLines(v) => (None, Some(v.len())),
                        _ => (None, None),
                    };
                    ExpectationInfo {
                        index: i + 1,
                        expectation_type: "response".into(),
                        content: c,
                        message_count: mc,
                        comparison_options: ComparisonOptions {
                            partial: s.inline_options.partial,
                            redact: s.inline_options.redact.clone(),
                            tolerance: s.inline_options.tolerance,
                            unordered_arrays: s.inline_options.unordered_arrays,
                            with_asserts: s.inline_options.with_asserts,
                        },
                        line_start: s.start_line,
                        line_end: s.end_line,
                    }
                })
                .collect()
        } else if let Some(s) = err_sect {
            let c = match &s.content {
                SectionContent::Json(j) => Some(j.clone()),
                _ => None,
            };
            vec![ExpectationInfo {
                index: 1,
                expectation_type: "error".into(),
                content: c,
                message_count: None,
                comparison_options: ComparisonOptions {
                    partial: s.inline_options.partial,
                    redact: s.inline_options.redact.clone(),
                    tolerance: s.inline_options.tolerance,
                    unordered_arrays: s.inline_options.unordered_arrays,
                    with_asserts: s.inline_options.with_asserts,
                },
                line_start: s.start_line,
                line_end: s.end_line,
            }]
        } else {
            vec![]
        };

        let assertions: Vec<AssertionInfo> = doc.sections_by_type(SectionType::Asserts).iter().enumerate().map(|(i, s)| {
            let asserts = match &s.content { SectionContent::Assertions(lines) => lines.iter().map(|line| apif_optimizer::rewrite_assertion_expression_fixed_point_if_changed_with_level(line, OptimizeLevel::Safe).unwrap_or_else(|| line.clone())).collect(), _ => vec![] };
            AssertionInfo { index: i + 1, assertions: asserts, line_start: s.start_line, line_end: s.end_line, response_index: None }
        }).collect();

        let extractions: Vec<ExtractionInfo> = doc
            .sections_by_type(SectionType::Extract)
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let vars = match &s.content {
                    SectionContent::Extract(v) => v.clone(),
                    _ => HashMap::new(),
                };
                ExtractionInfo {
                    index: i + 1,
                    variables: vars,
                    line_start: s.start_line,
                    line_end: s.end_line,
                    response_index: None,
                }
            })
            .collect();

        let has_jl = resp_sects
            .iter()
            .any(|s| matches!(&s.content, SectionContent::JsonLines(v) if v.len() > 1));
        let rpc = infer_rpc_mode(&requests, &expectations, err_sect.is_some(), has_jl);
        let rpc_name = match &rpc {
            RpcMode::Unary => "Unary",
            RpcMode::UnaryError => "Unary Error",
            RpcMode::ServerStreaming { .. } => "Server Streaming",
            RpcMode::ClientStreaming { .. } => "Client Streaming",
            RpcMode::BidirectionalStreaming { .. } => "Bidirectional Streaming",
            RpcMode::Unknown => "Unknown",
        };

        let req_count = requests.len();
        let resp_count = expectations
            .iter()
            .filter(|e| e.expectation_type == "response")
            .count();
        let err_count = expectations
            .iter()
            .filter(|e| e.expectation_type == "error")
            .count();
        let has_error = expectations.iter().any(|e| e.expectation_type == "error");
        let asrt_count = assertions.len();
        let ext_count = extractions.len();
        ExecutionPlan {
            file_path: doc.file_path.clone(),
            connection: conn,
            target,
            headers,
            requests,
            expectations,
            assertions,
            extractions,
            rpc_mode: rpc,
            summary: ExecutionSummary {
                total_requests: req_count,
                total_responses: resp_count,
                total_errors: err_count,
                error_expected: has_error,
                assertion_blocks: asrt_count,
                variable_extractions: ext_count,
                rpc_mode_name: rpc_name.into(),
            },
        }
    }
}

fn infer_rpc_mode(
    reqs: &[RequestInfo],
    exps: &[ExpectationInfo],
    has_err: bool,
    has_jl: bool,
) -> RpcMode {
    let rc = reqs.len();
    let ec = exps
        .iter()
        .filter(|e| e.expectation_type == "response")
        .count();
    if has_err {
        RpcMode::UnaryError
    } else if has_jl || ec > 1 {
        if rc > 1 {
            RpcMode::BidirectionalStreaming {
                request_count: rc,
                response_count: ec,
            }
        } else {
            RpcMode::ServerStreaming { response_count: ec }
        }
    } else if rc > 1 {
        RpcMode::ClientStreaming { request_count: rc }
    } else if rc == 1 && ec == 1 {
        RpcMode::Unary
    } else {
        RpcMode::Unknown
    }
}

pub fn infer_rpc_mode_for_section_types(doc: &GctfDocument) -> RpcMode {
    let rc = doc.sections_by_type(SectionType::Request).len();
    let rs = doc.sections_by_type(SectionType::Response);
    let has_jl = rs
        .iter()
        .any(|s| matches!(&s.content, SectionContent::JsonLines(v) if v.len() > 1));
    let has_err = doc.first_section(SectionType::Error).is_some();
    if has_err {
        RpcMode::Unary
    } else if has_jl || rs.len() > 1 {
        if rc > 1 {
            RpcMode::BidirectionalStreaming {
                request_count: rc,
                response_count: rs.len(),
            }
        } else {
            RpcMode::ServerStreaming {
                response_count: rs.len(),
            }
        }
    } else if rc > 1 {
        RpcMode::ClientStreaming { request_count: rc }
    } else {
        RpcMode::Unary
    }
}
