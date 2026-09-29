use crate::optimizer;
use serde::{Deserialize, Serialize};

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
        has_content: bool,
        options: ResponseOptions,
    },

    Extract {
        variables: Vec<String>,
        source_response_index: Option<usize>,
        line_range: (usize, usize),
    },

    Extracted {
        variables: Vec<String>,
    },

    Assert {
        count: usize,
        target_response_index: Option<usize>,
        line_range: (usize, usize),
    },

    Asserted {
        passed: usize,
        failed: usize,
    },

    Error {
        code: i32,
        message: String,
    },

    Complete {
        total_requests: usize,
        total_responses: usize,
        total_extractions: usize,
        total_assertions: usize,
        backends_used: Vec<String>,
    },

    SemanticAnalysis {
        type_mismatches: Vec<SemanticError>,
        unknown_plugins: Vec<SemanticError>,
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

impl From<&crate::execution::ComparisonOptions> for ResponseOptions {
    fn from(opts: &crate::execution::ComparisonOptions) -> Self {
        Self {
            partial: opts.partial,
            redact: opts.redact.clone(),
            has_tolerance: opts.tolerance.is_some(),
            unordered_arrays: opts.unordered_arrays,
            with_asserts: opts.with_asserts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub file_path: String,
    pub events: Vec<WorkflowEvent>,
    pub summary: WorkflowSummary,
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

impl Workflow {
    #[must_use]
    pub fn has_streaming(&self) -> bool {
        let request_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::RequestSent { .. }))
            .count();
        let response_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ResponseReceived { .. }))
            .count();
        request_count > 1 || response_count > 1
    }

    pub fn rpc_mode_name(&self) -> &str {
        let request_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::RequestSent { .. }))
            .count();
        let response_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ResponseReceived { .. }))
            .count();
        if request_count > 1 && response_count == 1 {
            return "Client Streaming";
        }
        if request_count == 1 && response_count > 1 {
            return "Server Streaming";
        }
        if request_count > 1 && response_count > 1 {
            return "Bidi Streaming";
        }
        "Unary"
    }
    fn message_counts(plan: &crate::execution::ExecutionPlan) -> (usize, usize) {
        let requests = plan
            .requests
            .iter()
            .map(|r| {
                if r.content_type == "json-lines" {
                    r.content.as_array().map_or(1, |v| v.len().max(1))
                } else {
                    1
                }
            })
            .sum();
        let responses = plan
            .expectations
            .iter()
            .filter(|e| e.expectation_type == "response")
            .map(|e| e.message_count.unwrap_or(1).max(1))
            .sum();
        (requests, responses)
    }

    pub fn from_plan(plan: &crate::execution::ExecutionPlan) -> Self {
        let mut events = Vec::new();
        let backend = plan.connection.backend.clone();

        events.push(WorkflowEvent::TestLoaded {
            file_path: plan.file_path.clone(),
        });

        events.push(WorkflowEvent::Connect {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });
        events.push(WorkflowEvent::Connected {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });

        events.push(WorkflowEvent::LoadDescriptors {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
        });
        events.push(WorkflowEvent::DescriptorsLoaded {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
            method_count: 1,
        });

        for request in &plan.requests {
            let messages = if request.content_type == "json-lines" {
                request.content.as_array().map_or(1, |v| v.len().max(1))
            } else {
                1
            };
            for _ in 0..messages {
                events.push(WorkflowEvent::SendRequest {
                    backend: backend.clone(),
                    request_index: request.index,
                    content_type: request.content_type.clone(),
                    line_range: (request.line_start, request.line_end),
                });
                events.push(WorkflowEvent::RequestSent {
                    backend: backend.clone(),
                    request_index: request.index,
                });
            }
        }

        for expectation in &plan.expectations {
            let messages = expectation.message_count.unwrap_or(1).max(1);
            for _ in 0..messages {
                events.push(WorkflowEvent::ReceiveResponse {
                    backend: backend.clone(),
                    response_index: expectation.index,
                    expectation_type: expectation.expectation_type.clone(),
                });
            }

            if expectation.expectation_type == "error"
                && let Some(content) = &expectation.content
            {
                let code = content.get("code").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
                let message = content
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                events.push(WorkflowEvent::Error { code, message });
            }

            for _ in 0..messages {
                events.push(WorkflowEvent::ResponseReceived {
                    backend: backend.clone(),
                    response_index: expectation.index,
                    has_content: expectation.content.is_some(),
                    options: ResponseOptions::from(&expectation.comparison_options),
                });
            }
        }

        for extraction in &plan.extractions {
            events.push(WorkflowEvent::Extract {
                variables: extraction.variables.keys().cloned().collect(),
                source_response_index: extraction.response_index,
                line_range: (extraction.line_start, extraction.line_end),
            });
            events.push(WorkflowEvent::Extracted {
                variables: extraction.variables.keys().cloned().collect(),
            });
        }

        for assertion in &plan.assertions {
            events.push(WorkflowEvent::Assert {
                count: assertion.assertions.len(),
                target_response_index: assertion.response_index,
                line_range: (assertion.line_start, assertion.line_end),
            });
            events.push(WorkflowEvent::Asserted {
                passed: assertion.assertions.len(),
                failed: 0,
            });
        }

        events.push(WorkflowEvent::Complete {
            total_requests: plan.requests.len(),
            total_responses: plan.expectations.len(),
            total_extractions: plan.extractions.len(),
            total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
            backends_used: vec![backend.clone()],
        });

        let (total_requests, total_responses) = Self::message_counts(plan);
        let has_streaming = total_requests > 1 || total_responses > 1;
        let has_bidi_streaming = total_requests > 1 && total_responses > 1;

        Self {
            file_path: plan.file_path.clone(),
            events,
            summary: WorkflowSummary {
                total_requests,
                total_responses,
                total_extractions: plan.extractions.len(),
                total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
                backends: vec![backend],
                rpc_mode: plan.summary.rpc_mode_name.clone(),
                has_streaming,
                has_bidi_streaming,
            },
        }
    }

    pub fn events_by_type(&self, event_type: &str) -> Vec<&WorkflowEvent> {
        self.events
            .iter()
            .filter(|e| {
                matches!(
                    (e, event_type),
                    (WorkflowEvent::TestLoaded { .. }, "TestLoaded")
                        | (WorkflowEvent::Connect { .. }, "Connect")
                        | (WorkflowEvent::Connected { .. }, "Connected")
                        | (WorkflowEvent::LoadDescriptors { .. }, "LoadDescriptors")
                        | (WorkflowEvent::DescriptorsLoaded { .. }, "DescriptorsLoaded")
                        | (WorkflowEvent::SendRequest { .. }, "SendRequest")
                        | (WorkflowEvent::RequestSent { .. }, "RequestSent")
                        | (WorkflowEvent::ReceiveResponse { .. }, "ReceiveResponse")
                        | (WorkflowEvent::ResponseReceived { .. }, "ResponseReceived")
                        | (WorkflowEvent::Extract { .. }, "Extract")
                        | (WorkflowEvent::Extracted { .. }, "Extracted")
                        | (WorkflowEvent::Assert { .. }, "Assert")
                        | (WorkflowEvent::Asserted { .. }, "Asserted")
                        | (WorkflowEvent::Error { .. }, "Error")
                        | (WorkflowEvent::Complete { .. }, "Complete")
                        | (WorkflowEvent::SemanticAnalysis { .. }, "SemanticAnalysis")
                        | (WorkflowEvent::OptimizationFound { .. }, "OptimizationFound")
                        | (WorkflowEvent::ValidationResult { .. }, "ValidationResult")
                )
            })
            .collect()
    }

    pub fn requests(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("SendRequest")
    }

    pub fn responses(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("ResponseReceived")
    }

    pub fn extractions(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("Extract")
    }

    pub fn assertions(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("Assert")
    }

    pub fn semantic_analysis(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("SemanticAnalysis")
    }

    pub fn optimization_hints(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("OptimizationFound")
    }

    pub fn validation_results(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("ValidationResult")
    }

    pub fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();

        if !matches!(self.events.first(), Some(WorkflowEvent::TestLoaded { .. })) {
            errors.push("Workflow must start with TestLoaded event".to_string());
        }

        if !self
            .events
            .iter()
            .any(|e| matches!(e, WorkflowEvent::Connect { .. }))
        {
            errors.push("Workflow must have Connect event".to_string());
        }

        if !matches!(self.events.last(), Some(WorkflowEvent::Complete { .. })) {
            errors.push("Workflow must end with Complete event".to_string());
        }

        let send_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::SendRequest { .. }))
            .count();
        let sent_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::RequestSent { .. }))
            .count();
        if send_count != sent_count {
            errors.push(format!(
                "Mismatched request events: {} sends, {} sent",
                send_count, sent_count
            ));
        }

        let receive_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ReceiveResponse { .. }))
            .count();
        let received_count = self
            .events
            .iter()
            .filter(|e| matches!(e, WorkflowEvent::ResponseReceived { .. }))
            .count();
        if receive_count != received_count {
            errors.push(format!(
                "Mismatched response events: {} receives, {} received",
                receive_count, received_count
            ));
        }

        ValidationResult {
            passed: errors.is_empty(),
            errors,
        }
    }

    pub fn analyze_streaming(&self) -> StreamingPattern {
        let mut pattern = StreamingPattern::Unary;

        let request_count = self.requests().len();
        let response_count = self.responses().len();

        let mut max_consecutive_requests = 0;
        let mut max_consecutive_responses = 0;
        let mut current_requests = 0;
        let mut current_responses = 0;

        for event in &self.events {
            match event {
                WorkflowEvent::RequestSent { .. } => {
                    current_requests += 1;
                    current_responses = 0;
                    max_consecutive_requests = max_consecutive_requests.max(current_requests);
                }
                WorkflowEvent::ResponseReceived { .. } => {
                    current_responses += 1;
                    current_requests = 0;
                    max_consecutive_responses = max_consecutive_responses.max(current_responses);
                }
                _ => {}
            }
        }

        if request_count > 1 && response_count > 1 {
            pattern = StreamingPattern::Bidirectional {
                request_count,
                response_count,
                max_consecutive_requests,
                max_consecutive_responses,
            };
        } else if request_count > 1 {
            pattern = StreamingPattern::ClientStreaming {
                request_count,
                max_consecutive_requests,
            };
        } else if response_count > 1 {
            pattern = StreamingPattern::ServerStreaming {
                response_count,
                max_consecutive_responses,
            };
        }

        pattern
    }

    pub fn from_document_with_analysis(doc: &crate::parser::GctfDocument) -> Workflow {
        let mut events = Vec::new();

        events.push(WorkflowEvent::TestLoaded {
            file_path: doc.file_path.clone(),
        });

        let type_mismatches: Vec<SemanticError> =
            crate::semantics::collect_assertion_type_mismatches(doc)
                .into_iter()
                .map(|m| SemanticError {
                    line: m.line,
                    rule_id: m.rule_id,
                    message: m.message,
                    expression: Some(m.expression),
                    plugin_name: None,
                })
                .collect();

        let unknown_plugins: Vec<SemanticError> =
            crate::semantics::collect_unknown_plugin_calls(doc)
                .into_iter()
                .map(|u| SemanticError {
                    line: u.line,
                    rule_id: u.rule_id,
                    message: u.message,
                    expression: Some(u.expression),
                    plugin_name: Some(u.plugin_name),
                })
                .collect();

        events.push(WorkflowEvent::SemanticAnalysis {
            type_mismatches,
            unknown_plugins,
        });

        let hints: Vec<OptimizationHint> = crate::optimizer::collect_assertion_optimizations(
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

        let validation_result = crate::parser::validate_document(doc);
        let passed = validation_result.is_ok();
        let errors = match validation_result {
            Err(e) => vec![e.to_string()],
            Ok(_) => vec![],
        };

        events.push(WorkflowEvent::ValidationResult { passed, errors });

        let plan = crate::execution::ExecutionPlan::from_document(doc);
        Self::add_execution_events_from_plan(&mut events, &plan);

        let (total_requests, total_responses) = Self::message_counts(&plan);
        let has_streaming = total_requests > 1 || total_responses > 1;
        let has_bidi_streaming = total_requests > 1 && total_responses > 1;

        Workflow {
            file_path: doc.file_path.clone(),
            events,
            summary: WorkflowSummary {
                total_requests,
                total_responses,
                total_extractions: plan.extractions.len(),
                total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
                backends: vec!["default".to_string()],
                rpc_mode: plan.summary.rpc_mode_name.clone(),
                has_streaming,
                has_bidi_streaming,
            },
        }
    }

    fn add_execution_events_from_plan(
        events: &mut Vec<WorkflowEvent>,
        plan: &crate::execution::ExecutionPlan,
    ) {
        let backend = "default".to_string();

        events.push(WorkflowEvent::Connect {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });
        events.push(WorkflowEvent::Connected {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });

        events.push(WorkflowEvent::LoadDescriptors {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
        });
        events.push(WorkflowEvent::DescriptorsLoaded {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
            method_count: 1,
        });

        for request in &plan.requests {
            let messages = if request.content_type == "json-lines" {
                request.content.as_array().map_or(1, |v| v.len().max(1))
            } else {
                1
            };
            for _ in 0..messages {
                events.push(WorkflowEvent::SendRequest {
                    backend: backend.clone(),
                    request_index: request.index,
                    content_type: request.content_type.clone(),
                    line_range: (request.line_start, request.line_end),
                });
                events.push(WorkflowEvent::RequestSent {
                    backend: backend.clone(),
                    request_index: request.index,
                });
            }
        }

        for expectation in &plan.expectations {
            let messages = expectation.message_count.unwrap_or(1).max(1);
            for _ in 0..messages {
                events.push(WorkflowEvent::ReceiveResponse {
                    backend: backend.clone(),
                    response_index: expectation.index,
                    expectation_type: expectation.expectation_type.clone(),
                });
            }

            if expectation.expectation_type == "error"
                && let Some(content) = &expectation.content
            {
                let code = content.get("code").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
                let message = content
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                events.push(WorkflowEvent::Error { code, message });
            }

            for _ in 0..expectation.message_count.unwrap_or(1).max(1) {
                events.push(WorkflowEvent::ResponseReceived {
                    backend: backend.clone(),
                    response_index: expectation.index,
                    has_content: expectation.content.is_some(),
                    options: ResponseOptions::from(&expectation.comparison_options),
                });
            }
        }

        for extraction in &plan.extractions {
            events.push(WorkflowEvent::Extract {
                variables: extraction.variables.keys().cloned().collect(),
                source_response_index: extraction.response_index,
                line_range: (extraction.line_start, extraction.line_end),
            });
            events.push(WorkflowEvent::Extracted {
                variables: extraction.variables.keys().cloned().collect(),
            });
        }

        for assertion in &plan.assertions {
            events.push(WorkflowEvent::Assert {
                count: assertion.assertions.len(),
                target_response_index: assertion.response_index,
                line_range: (assertion.line_start, assertion.line_end),
            });
            events.push(WorkflowEvent::Asserted {
                passed: assertion.assertions.len(),
                failed: 0,
            });
        }

        events.push(WorkflowEvent::Complete {
            total_requests: plan.requests.len(),
            total_responses: plan.expectations.len(),
            total_extractions: plan.extractions.len(),
            total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
            backends_used: vec![backend],
        });
    }
}

#[derive(Debug, Clone)]
pub enum StreamingPattern {
    Unary,
    ServerStreaming {
        response_count: usize,
        max_consecutive_responses: usize,
    },
    ClientStreaming {
        request_count: usize,
        max_consecutive_requests: usize,
    },
    Bidirectional {
        request_count: usize,
        response_count: usize,
        max_consecutive_requests: usize,
        max_consecutive_responses: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub passed: bool,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{
        ConnectionInfo, ExecutionPlan, ExecutionSummary, ExpectationInfo, RequestInfo, TargetInfo,
    };
    use serde_json::json;

    fn create_test_plan() -> ExecutionPlan {
        ExecutionPlan {
            file_path: "test.gctf".to_string(),
            connection: ConnectionInfo {
                address: "localhost:50051".to_string(),
                source: "test".to_string(),
                backend: "default".to_string(),
            },
            target: TargetInfo {
                endpoint: "test.Service/Method".to_string(),
                package: Some("test".to_string()),
                service: Some("Service".to_string()),
                method: Some("Method".to_string()),
            },
            headers: None,
            requests: vec![RequestInfo {
                skipped: false,
                index: 1,
                content: json!({"key": "value"}),
                content_type: "json".to_string(),
                line_start: 5,
                line_end: 8,
            }],
            expectations: vec![ExpectationInfo {
                skipped: false,
                index: 1,
                expectation_type: "response".to_string(),
                content: Some(json!({"result": "ok"})),
                message_count: None,
                comparison_options: Default::default(),
                line_start: 10,
                line_end: 13,
            }],
            assertions: vec![],
            extractions: vec![],
            rpc_mode: crate::execution::RpcMode::Unary,
            summary: ExecutionSummary {
                rpc_mode_name: "Unary".to_string(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn workflow_from_plan() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);

        assert_eq!(workflow.file_path, "test.gctf");
        assert_eq!(workflow.summary.rpc_mode, "Unary");
        assert!(workflow.events.len() >= 10);
    }

    #[test]
    fn workflow_validate() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);
        let result = workflow.validate();

        assert!(result.passed, "Validation failed: {:?}", result.errors);
    }

    #[test]
    fn workflow_events_by_type() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);

        let requests = workflow.requests();
        assert_eq!(requests.len(), 1);

        let responses = workflow.responses();
        assert_eq!(responses.len(), 1);
    }

    #[test]
    fn workflow_streaming_analysis_unary() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);
        let pattern = workflow.analyze_streaming();

        assert!(matches!(pattern, StreamingPattern::Unary));
    }

    #[test]
    fn workflow_streaming_analysis_server() {
        let mut plan = create_test_plan();
        plan.expectations.push(ExpectationInfo {
            skipped: false,
            index: 2,
            expectation_type: "response".to_string(),
            content: Some(json!({"result": "ok2"})),
            message_count: None,
            comparison_options: Default::default(),
            line_start: 15,
            line_end: 18,
        });

        let workflow = Workflow::from_plan(&plan);
        let pattern = workflow.analyze_streaming();

        assert!(matches!(pattern, StreamingPattern::ServerStreaming { .. }));
    }
}
