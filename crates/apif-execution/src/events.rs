use crate::model::{ComparisonOptions, ExecutionPlan};
use apif_ast::GctfDocument;
use apif_optimizer as optimizer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamingPattern {
    Sequential,
    Parallel,
    Interleaved,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationResult {
    pub passed: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkflowEvent {
    TestLoaded {
        file_path: String,
    },
    Connect {
        backend: String,
        address: String,
    },
    Connected {
        backend: String,
        address: String,
    },
    LoadDescriptors {
        backend: String,
        service: String,
    },
    DescriptorsLoaded {
        backend: String,
        service: String,
        method_count: usize,
    },
    SendRequest {
        backend: String,
        request_index: usize,
        content_type: String,
        line_range: (usize, usize),
    },
    RequestSent {
        backend: String,
        request_index: usize,
    },
    ReceiveResponse {
        backend: String,
        response_index: usize,
        expectation_type: String,
    },
    ResponseReceived {
        backend: String,
        response_index: usize,
    },
    Assertion {
        backend: String,
        expression: String,
        passed: bool,
        line: usize,
    },
    Extraction {
        backend: String,
        variable: String,
        value: String,
        line: usize,
    },
    Error {
        backend: String,
        message: String,
    },
    TrailersReceived {
        backend: String,
        trailers: std::collections::HashMap<String, String>,
    },
    Done {
        backend: String,
        duration_ms: u64,
    },
    SemanticAnalysis {
        type_mismatches: Vec<SemanticError>,
        unknown_plugins: Vec<String>,
    },
    OptimizationFound {
        hints: Vec<OptimizationHint>,
    },
    ValidationResult {
        passed: bool,
        errors: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticError {
    pub line: usize,
    pub rule_id: String,
    pub message: String,
    pub expression: Option<String>,
    pub plugin_name: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizationHint {
    pub line: usize,
    pub rule_id: String,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseOptions {
    pub partial: bool,
    pub redact: Vec<String>,
    pub has_tolerance: bool,
    pub unordered_arrays: bool,
    pub with_asserts: bool,
}

impl From<&ComparisonOptions> for ResponseOptions {
    fn from(o: &ComparisonOptions) -> Self {
        Self {
            partial: o.partial,
            redact: o.redact.clone(),
            has_tolerance: o.tolerance.is_some(),
            unordered_arrays: o.unordered_arrays,
            with_asserts: o.with_asserts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub file_path: String,
    pub events: Vec<WorkflowEvent>,
    pub summary: WorkflowSummary,
}
impl Workflow {
    pub fn has_streaming(&self) -> bool {
        let rc = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::RequestSent { .. }))
            .count();
        let ec = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ResponseReceived { .. }))
            .count();
        rc > 1 || ec > 1
    }
    pub fn rpc_mode_name(&self) -> &str {
        let rc = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::RequestSent { .. }))
            .count();
        let ec = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ResponseReceived { .. }))
            .count();
        if rc > 1 && ec == 1 {
            return "Client Streaming";
        }
        if rc == 1 && ec > 1 {
            return "Server Streaming";
        }
        if rc > 1 && ec > 1 {
            return "Bidi Streaming";
        }
        "Unary"
    }
    pub fn from_plan(plan: &ExecutionPlan) -> Self {
        let mut events = vec![
            WorkflowEvent::TestLoaded {
                file_path: plan.file_path.clone(),
            },
            WorkflowEvent::Connect {
                backend: plan.connection.backend.clone(),
                address: plan.connection.address.clone(),
            },
            WorkflowEvent::Connected {
                backend: plan.connection.backend.clone(),
                address: plan.connection.address.clone(),
            },
        ];
        for r in &plan.requests {
            events.push(WorkflowEvent::SendRequest {
                backend: plan.connection.backend.clone(),
                request_index: r.index,
                content_type: r.content_type.clone(),
                line_range: (r.line_start, r.line_end),
            });
            events.push(WorkflowEvent::RequestSent {
                backend: plan.connection.backend.clone(),
                request_index: r.index,
            });
        }
        for e in &plan.expectations {
            events.push(WorkflowEvent::ReceiveResponse {
                backend: plan.connection.backend.clone(),
                response_index: e.index,
                expectation_type: e.expectation_type.clone(),
            });
            events.push(WorkflowEvent::ResponseReceived {
                backend: plan.connection.backend.clone(),
                response_index: e.index,
            });
        }
        for a in &plan.assertions {
            events.push(WorkflowEvent::Assertion {
                backend: plan.connection.backend.clone(),
                expression: a.assertions.first().cloned().unwrap_or_default(),
                passed: true,
                line: a.line_start,
            });
        }
        for ex in &plan.extractions {
            for (k, v) in &ex.variables {
                events.push(WorkflowEvent::Extraction {
                    backend: plan.connection.backend.clone(),
                    variable: k.clone(),
                    value: v.clone(),
                    line: ex.line_start,
                });
            }
        }
        events.push(WorkflowEvent::Done {
            backend: plan.connection.backend.clone(),
            duration_ms: 0,
        });

        let summary = WorkflowSummary {
            total_requests: plan.requests.len(),
            total_responses: plan
                .expectations
                .iter()
                .filter(|e| e.expectation_type == "response")
                .count(),
            total_extractions: plan.extractions.len(),
            total_assertions: plan.assertions.len(),
            backends: vec![plan.connection.backend.clone()],
            rpc_mode: plan.summary.rpc_mode_name.clone(),
            has_streaming: plan.requests.len() > 1 || plan.expectations.len() > 1,
            has_bidi_streaming: plan.requests.len() > 1 && plan.expectations.len() > 1,
        };
        Self {
            file_path: plan.file_path.clone(),
            events,
            summary,
        }
    }

    pub fn from_document_with_analysis(doc: &GctfDocument) -> Self {
        let file_path = doc.file_path.clone();
        let mut events = vec![WorkflowEvent::TestLoaded {
            file_path: file_path.clone(),
        }];

        events.push(WorkflowEvent::Connect {
            backend: "default".into(),
            address: String::new(),
        });
        events.push(WorkflowEvent::Connected {
            backend: "default".into(),
            address: String::new(),
        });

        if let Some(s) = doc.first_section(apif_ast::SectionType::Endpoint) {
            if let apif_ast::SectionContent::Single(e) = &s.content {
                events.push(WorkflowEvent::LoadDescriptors {
                    backend: "default".into(),
                    service: e.clone(),
                });
            }
        }
        events.push(WorkflowEvent::DescriptorsLoaded {
            backend: "default".into(),
            service: String::new(),
            method_count: 0,
        });

        for (i, s) in doc
            .sections_by_type(apif_ast::SectionType::Request)
            .iter()
            .enumerate()
        {
            let ct = match &s.content {
                apif_ast::SectionContent::Json(_) => "json",
                apif_ast::SectionContent::JsonLines(_) => "json-lines",
                _ => "unknown",
            };
            events.push(WorkflowEvent::SendRequest {
                backend: "default".into(),
                request_index: i + 1,
                content_type: ct.into(),
                line_range: (s.start_line, s.end_line),
            });
            events.push(WorkflowEvent::RequestSent {
                backend: "default".into(),
                request_index: i + 1,
            });
        }
        for (i, _s) in doc
            .sections_by_type(apif_ast::SectionType::Response)
            .iter()
            .enumerate()
        {
            events.push(WorkflowEvent::ReceiveResponse {
                backend: "default".into(),
                response_index: i + 1,
                expectation_type: "response".into(),
            });
            events.push(WorkflowEvent::ResponseReceived {
                backend: "default".into(),
                response_index: i + 1,
            });
        }
        if let Some(s) = doc.first_section(apif_ast::SectionType::Error) {
            events.push(WorkflowEvent::Error {
                backend: "default".into(),
                message: format!("Error at line {}", s.start_line),
            });
        }
        for s in doc.sections_by_type(apif_ast::SectionType::Asserts) {
            if let apif_ast::SectionContent::Assertions(lines) = &s.content {
                for line in lines {
                    events.push(WorkflowEvent::Assertion {
                        backend: "default".into(),
                        expression: line.clone(),
                        passed: true,
                        line: s.start_line,
                    });
                }
            }
        }
        for s in doc.sections_by_type(apif_ast::SectionType::Extract) {
            if let apif_ast::SectionContent::Extract(vars) = &s.content {
                for (k, v) in vars {
                    events.push(WorkflowEvent::Extraction {
                        backend: "default".into(),
                        variable: k.clone(),
                        value: v.clone(),
                        line: s.start_line,
                    });
                }
            }
        }

        if let Some(mismatches) = doc
            .first_section(apif_ast::SectionType::Meta)
            .and_then(|_| {
                let tm = apif_semantics::collect_assertion_type_mismatches(doc);
                let up = apif_semantics::collect_unknown_plugin_calls(doc);
                if tm.is_empty() && up.is_empty() {
                    None
                } else {
                    Some((
                        tm.into_iter()
                            .map(|u| SemanticError {
                                line: u.line,
                                rule_id: u.rule_id.clone(),
                                message: u.message,
                                expression: Some(u.expression),
                                plugin_name: Some(u.rule_id.clone()),
                            })
                            .collect(),
                        up.into_iter().map(|u| u.plugin_name).collect(),
                    ))
                }
            })
        {
            events.push(WorkflowEvent::SemanticAnalysis {
                type_mismatches: mismatches.0,
                unknown_plugins: mismatches.1,
            });
        }

        let hints: Vec<OptimizationHint> = apif_optimizer::collect_assertion_optimizations(
            doc,
            optimizer::OptimizeLevel::Advisory,
        )
        .into_iter()
        .map(|h| OptimizationHint {
            line: h.line,
            rule_id: h.rule_id.to_string(),
            before: h.before,
            after: h.after,
        })
        .collect();
        if !hints.is_empty() {
            events.push(WorkflowEvent::OptimizationFound { hints });
        }

        let validation = apif_parser::validate_document(doc);
        events.push(WorkflowEvent::ValidationResult {
            passed: validation.is_ok(),
            errors: validation
                .err()
                .into_iter()
                .map(|e| e.to_string())
                .collect(),
        });

        let plan = ExecutionPlan::from_document(doc);
        events.push(WorkflowEvent::Done {
            backend: "default".into(),
            duration_ms: 0,
        });

        let summary = WorkflowSummary {
            total_requests: plan.requests.len(),
            total_responses: plan
                .expectations
                .iter()
                .filter(|e| e.expectation_type == "response")
                .count(),
            total_extractions: plan.extractions.len(),
            total_assertions: plan.assertions.len(),
            backends: vec!["default".into()],
            rpc_mode: plan.summary.rpc_mode_name.clone(),
            has_streaming: plan.requests.len() > 1 || plan.expectations.len() > 1,
            has_bidi_streaming: plan.requests.len() > 1 && plan.expectations.len() > 1,
        };
        Self {
            file_path,
            events,
            summary,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowSummary {
    pub total_requests: usize,
    pub total_responses: usize,
    pub total_extractions: usize,
    pub total_assertions: usize,
    pub backends: Vec<String>,
    pub rpc_mode: String,
    pub has_streaming: bool,
    pub has_bidi_streaming: bool,
}
