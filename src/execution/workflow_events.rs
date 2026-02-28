// Workflow events - semantic execution flow from ExecutionPlan
// Supports: N requests, N responses, multiple backends, interleaved streaming

use serde::{Deserialize, Serialize};

/// Workflow event - represents a semantic step in test execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowEvent {
    /// Test file loaded
    TestLoaded { file_path: String },

    /// Connecting to a backend service
    Connect { backend: String, address: String },

    /// Connected to backend
    Connected { backend: String, address: String },

    /// Loading service descriptors
    LoadDescriptors { backend: String, service: String },

    /// Descriptors loaded
    DescriptorsLoaded {
        backend: String,
        service: String,
        method_count: usize,
    },

    /// Sending a request
    SendRequest {
        backend: String,
        request_index: usize,
        content_type: String,
        line_range: (usize, usize),
    },

    /// Request sent
    RequestSent {
        backend: String,
        request_index: usize,
    },

    /// Receiving a response
    ReceiveResponse {
        backend: String,
        response_index: usize,
        expectation_type: String,
    },

    /// Response received
    ResponseReceived {
        backend: String,
        response_index: usize,
        has_content: bool,
        options: ResponseOptions,
    },

    /// Extracting variables from response
    Extract {
        variables: Vec<String>,
        source_response_index: Option<usize>,
        line_range: (usize, usize),
    },

    /// Variables extracted
    Extracted { variables: Vec<String> },

    /// Running assertions
    Assert {
        count: usize,
        target_response_index: Option<usize>,
        line_range: (usize, usize),
    },

    /// Assertions completed
    Asserted { passed: usize, failed: usize },

    /// Error occurred
    Error { code: i32, message: String },

    /// Test execution complete
    Complete {
        total_requests: usize,
        total_responses: usize,
        total_extractions: usize,
        total_assertions: usize,
        backends_used: Vec<String>,
    },
}

/// Response options from inline options
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

/// Workflow - sequence of events for a test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub file_path: String,
    pub events: Vec<WorkflowEvent>,
    pub summary: WorkflowSummary,
}

/// Workflow summary
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
    /// Build workflow from ExecutionPlan
    pub fn from_plan(plan: &crate::execution::ExecutionPlan) -> Self {
        let mut events = Vec::new();
        let backend = "default".to_string(); // In future, support multiple backends

        // Test loaded
        events.push(WorkflowEvent::TestLoaded {
            file_path: plan.file_path.clone(),
        });

        // Connect to backend
        events.push(WorkflowEvent::Connect {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });
        events.push(WorkflowEvent::Connected {
            backend: backend.clone(),
            address: plan.connection.address.clone(),
        });

        // Load descriptors
        events.push(WorkflowEvent::LoadDescriptors {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
        });
        events.push(WorkflowEvent::DescriptorsLoaded {
            backend: backend.clone(),
            service: plan.target.endpoint.clone(),
            method_count: 1,
        });

        // Process requests, responses, extractions, assertions in order
        // For now, we use the order from ExecutionPlan
        // In future, we'll track explicit ordering from .gctf file

        // Requests
        for request in &plan.requests {
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

        // Expectations (responses or error)
        for expectation in &plan.expectations {
            events.push(WorkflowEvent::ReceiveResponse {
                backend: backend.clone(),
                response_index: expectation.index,
                expectation_type: expectation.expectation_type.clone(),
            });

            // If this is an error expectation, add Error event
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

            events.push(WorkflowEvent::ResponseReceived {
                backend: backend.clone(),
                response_index: expectation.index,
                has_content: expectation.content.is_some(),
                options: ResponseOptions::from(&expectation.comparison_options),
            });
        }

        // Extractions
        for extraction in &plan.extractions {
            events.push(WorkflowEvent::Extract {
                variables: extraction.variables.keys().cloned().collect(),
                source_response_index: None, // In future, track which response
                line_range: (extraction.line_start, extraction.line_end),
            });
            events.push(WorkflowEvent::Extracted {
                variables: extraction.variables.keys().cloned().collect(),
            });
        }

        // Assertions
        for assertion in &plan.assertions {
            events.push(WorkflowEvent::Assert {
                count: assertion.assertions.len(),
                target_response_index: None, // In future, track which response
                line_range: (assertion.line_start, assertion.line_end),
            });
            events.push(WorkflowEvent::Asserted {
                passed: assertion.assertions.len(),
                failed: 0,
            });
        }

        // Complete
        events.push(WorkflowEvent::Complete {
            total_requests: plan.requests.len(),
            total_responses: plan.expectations.len(),
            total_extractions: plan.extractions.len(),
            total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
            backends_used: vec![backend.clone()],
        });

        // Detect streaming mode
        let has_streaming = plan.requests.len() > 1 || plan.expectations.len() > 1;
        let has_bidi_streaming = plan.requests.len() > 1 && plan.expectations.len() > 1;

        Self {
            file_path: plan.file_path.clone(),
            events,
            summary: WorkflowSummary {
                total_requests: plan.summary.total_requests,
                total_responses: plan.summary.total_responses,
                total_extractions: plan.extractions.len(),
                total_assertions: plan.assertions.iter().map(|a| a.assertions.len()).sum(),
                backends: vec![backend],
                rpc_mode: plan.summary.rpc_mode_name.clone(),
                has_streaming,
                has_bidi_streaming,
            },
        }
    }

    /// Get events by type
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
                )
            })
            .collect()
    }

    /// Get request events
    pub fn requests(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("SendRequest")
    }

    /// Get response events
    pub fn responses(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("ResponseReceived")
    }

    /// Get extraction events
    pub fn extractions(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("Extract")
    }

    /// Get assertion events
    pub fn assertions(&self) -> Vec<&WorkflowEvent> {
        self.events_by_type("Assert")
    }

    /// Validate workflow structure
    pub fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();

        // Must start with TestLoaded
        if !matches!(self.events.first(), Some(WorkflowEvent::TestLoaded { .. })) {
            errors.push("Workflow must start with TestLoaded event".to_string());
        }

        // Must have Connect
        if !self
            .events
            .iter()
            .any(|e| matches!(e, WorkflowEvent::Connect { .. }))
        {
            errors.push("Workflow must have Connect event".to_string());
        }

        // Must end with Complete
        if !matches!(self.events.last(), Some(WorkflowEvent::Complete { .. })) {
            errors.push("Workflow must end with Complete event".to_string());
        }

        // Each SendRequest should be followed by RequestSent
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

        // Each ReceiveResponse should be followed by ResponseReceived
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

    /// Analyze streaming pattern
    pub fn analyze_streaming(&self) -> StreamingPattern {
        let mut pattern = StreamingPattern::Unary;

        let request_count = self.requests().len();
        let response_count = self.responses().len();

        // Analyze event interleaving
        let mut max_consecutive_requests = 0;
        let mut max_consecutive_responses = 0;
        let mut current_requests = 0;
        let mut current_responses = 0;

        for event in &self.events {
            match event {
                WorkflowEvent::SendRequest { .. } | WorkflowEvent::RequestSent { .. } => {
                    current_requests += 1;
                    current_responses = 0;
                    max_consecutive_requests = max_consecutive_requests.max(current_requests);
                }
                WorkflowEvent::ReceiveResponse { .. } | WorkflowEvent::ResponseReceived { .. } => {
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
}

/// Streaming pattern analysis
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

/// Validation result
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
            },
            target: TargetInfo {
                endpoint: "test.Service/Method".to_string(),
                package: Some("test".to_string()),
                service: Some("Service".to_string()),
                method: Some("Method".to_string()),
            },
            headers: None,
            requests: vec![RequestInfo {
                index: 1,
                content: json!({"key": "value"}),
                content_type: "json".to_string(),
                line_start: 5,
                line_end: 8,
            }],
            expectations: vec![ExpectationInfo {
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
    fn test_workflow_from_plan() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);

        assert_eq!(workflow.file_path, "test.gctf");
        assert_eq!(workflow.summary.rpc_mode, "Unary");
        assert!(workflow.events.len() >= 10);
    }

    #[test]
    fn test_workflow_validate() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);
        let result = workflow.validate();

        assert!(result.passed, "Validation failed: {:?}", result.errors);
    }

    #[test]
    fn test_workflow_events_by_type() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);

        let requests = workflow.requests();
        assert_eq!(requests.len(), 1);

        let responses = workflow.responses();
        assert_eq!(responses.len(), 1);
    }

    #[test]
    fn test_workflow_streaming_analysis_unary() {
        let plan = create_test_plan();
        let workflow = Workflow::from_plan(&plan);
        let pattern = workflow.analyze_streaming();

        assert!(matches!(pattern, StreamingPattern::Unary));
    }

    #[test]
    fn test_workflow_streaming_analysis_server() {
        let mut plan = create_test_plan();
        plan.expectations.push(ExpectationInfo {
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
