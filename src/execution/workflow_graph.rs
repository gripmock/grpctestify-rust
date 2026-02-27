// Workflow graph builder - builds execution graph from GCTF document

use crate::parser::ast::{Section, SectionContent, SectionType};

/// Workflow step in the execution graph
#[derive(Debug, Clone)]
pub struct WorkflowStep {
    pub step_number: usize,
    pub step_type: WorkflowStepType,
    pub section_line: usize,
    pub description: String,
}

/// Type of workflow step
#[derive(Debug, Clone)]
pub enum WorkflowStepType {
    Connect,
    SendRequest { request_index: usize },
    ReceiveResponse { response_index: usize },
    ReceiveError { error_index: usize },
    ValidateResponse { validation_index: usize },
    ValidateError { validation_index: usize },
    Extract { extraction_index: usize },
    Assert { assertion_index: usize },
}

impl WorkflowStep {
    pub fn format(&self) -> String {
        match &self.step_type {
            WorkflowStepType::Connect => format!("{}. Connect", self.step_number),
            WorkflowStepType::SendRequest { request_index } => {
                format!("{}. Send Request #{}", self.step_number, request_index)
            }
            WorkflowStepType::ReceiveResponse { response_index } => {
                format!("{}. Receive Response #{}", self.step_number, response_index)
            }
            WorkflowStepType::ReceiveError { error_index } => {
                format!("{}. Receive gRPC Error #{}", self.step_number, error_index)
            }
            WorkflowStepType::ValidateResponse { validation_index } => {
                format!(
                    "{}. Validate Response #{}",
                    self.step_number, validation_index
                )
            }
            WorkflowStepType::ValidateError { validation_index } => {
                format!("{}. Validate Error #{}", self.step_number, validation_index)
            }
            WorkflowStepType::Extract { extraction_index } => {
                format!(
                    "{}. Extract Variables #{}",
                    self.step_number, extraction_index
                )
            }
            WorkflowStepType::Assert { assertion_index } => {
                format!("{}. Run Assertions #{}", self.step_number, assertion_index)
            }
        }
    }
}

/// Build workflow graph from document sections
pub fn build_workflow_graph(sections: &[Section]) -> Vec<WorkflowStep> {
    let mut steps = Vec::new();
    let mut step_number = 1;

    // Always start with Connect
    if let Some(first_section) = sections.first() {
        steps.push(WorkflowStep {
            step_number,
            step_type: WorkflowStepType::Connect,
            section_line: first_section.start_line,
            description: "Connect to gRPC server".to_string(),
        });
        step_number += 1;
    }

    // Track indices for each section type
    let mut request_index = 0;
    let mut response_index = 0;
    let mut error_index = 0;
    let mut validation_index = 0;
    let mut extraction_index = 0;
    let mut assertion_index = 0;

    // Process sections in order
    for section in sections {
        match section.section_type {
            SectionType::Request => {
                request_index += 1;
                steps.push(WorkflowStep {
                    step_number,
                    step_type: WorkflowStepType::SendRequest { request_index },
                    section_line: section.start_line,
                    description: format!("Send request #{}", request_index),
                });
                step_number += 1;
            }
            SectionType::Response => {
                response_index += 1;
                steps.push(WorkflowStep {
                    step_number,
                    step_type: WorkflowStepType::ReceiveResponse { response_index },
                    section_line: section.start_line,
                    description: format!("Receive response #{}", response_index),
                });
                step_number += 1;

                // Add validation step if response has content or with_asserts
                if section.inline_options.with_asserts
                    || matches!(
                        &section.content,
                        SectionContent::Json(_) | SectionContent::JsonLines(_)
                    )
                {
                    validation_index += 1;
                    steps.push(WorkflowStep {
                        step_number,
                        step_type: WorkflowStepType::ValidateResponse { validation_index },
                        section_line: section.start_line,
                        description: format!("Validate response #{}", validation_index),
                    });
                    step_number += 1;
                }
            }
            SectionType::Error => {
                error_index += 1;
                steps.push(WorkflowStep {
                    step_number,
                    step_type: WorkflowStepType::ReceiveError { error_index },
                    section_line: section.start_line,
                    description: format!("Receive gRPC error #{}", error_index),
                });
                step_number += 1;

                // Add validation step for error
                validation_index += 1;
                steps.push(WorkflowStep {
                    step_number,
                    step_type: WorkflowStepType::ValidateError { validation_index },
                    section_line: section.start_line,
                    description: format!("Validate error #{}", validation_index),
                });
                step_number += 1;
            }
            SectionType::Extract => {
                if let SectionContent::Extract(extractions) = &section.content
                    && !extractions.is_empty()
                {
                    extraction_index += 1;
                    steps.push(WorkflowStep {
                        step_number,
                        step_type: WorkflowStepType::Extract { extraction_index },
                        section_line: section.start_line,
                        description: format!("Extract variables #{}", extraction_index),
                    });
                    step_number += 1;
                }
            }
            SectionType::Asserts => {
                if let SectionContent::Assertions(assertions) = &section.content
                    && !assertions.is_empty()
                {
                    assertion_index += 1;
                    steps.push(WorkflowStep {
                        step_number,
                        step_type: WorkflowStepType::Assert { assertion_index },
                        section_line: section.start_line,
                        description: format!("Run assertions #{}", assertion_index),
                    });
                    step_number += 1;
                }
            }
            _ => {}
        }
    }

    steps
}

/// Get workflow summary statistics
pub struct WorkflowSummary {
    pub total_requests: usize,
    pub total_responses: usize,
    pub total_errors: usize,
    pub total_extractions: usize,
    pub total_assertions: usize,
    pub has_streaming: bool,
}

pub fn get_workflow_summary(sections: &[Section]) -> WorkflowSummary {
    let total_requests = sections
        .iter()
        .filter(|s| s.section_type == SectionType::Request)
        .count();
    let total_responses = sections
        .iter()
        .filter(|s| s.section_type == SectionType::Response)
        .count();
    let total_errors = sections
        .iter()
        .filter(|s| s.section_type == SectionType::Error)
        .count();
    let total_extractions = sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .count();
    let total_assertions = sections
        .iter()
        .filter(|s| s.section_type == SectionType::Asserts)
        .count();

    // Detect streaming based on multiple requests/responses
    let has_streaming = total_requests > 1 || total_responses > 1;

    WorkflowSummary {
        total_requests,
        total_responses,
        total_errors,
        total_extractions,
        total_assertions,
        has_streaming,
    }
}

/// Get call type description based on workflow
pub fn get_call_type(summary: &WorkflowSummary) -> &'static str {
    if summary.total_errors > 0 && summary.total_requests == 1 && summary.total_responses == 0 {
        "Unary Call Expecting Error"
    } else if summary.total_requests == 1 && summary.total_responses > 1 {
        "Server Streaming Call"
    } else if summary.total_requests > 1 && summary.total_responses == 1 {
        "Client Streaming Call"
    } else if summary.total_requests > 1 && summary.total_responses > 1 {
        "Bidirectional Streaming Call"
    } else if summary.total_requests == 1 && summary.total_responses == 1 {
        "Standard Unary Call"
    } else {
        "Multi-Step Workflow"
    }
}
