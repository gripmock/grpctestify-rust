// Workflow summary and call type detection

use crate::parser::ast::{Section, SectionType};

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
