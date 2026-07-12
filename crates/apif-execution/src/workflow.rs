use apif_ast::{Section, SectionType};

pub struct WorkflowSummary {
    pub total_requests: usize,
    pub total_responses: usize,
    pub total_errors: usize,
    pub total_extractions: usize,
    pub total_assertions: usize,
    pub has_streaming: bool,
}
pub fn get_workflow_summary(sections: &[Section]) -> WorkflowSummary {
    WorkflowSummary {
        total_requests: sections
            .iter()
            .filter(|s| s.section_type == SectionType::Request)
            .count(),
        total_responses: sections
            .iter()
            .filter(|s| s.section_type == SectionType::Response)
            .count(),
        total_errors: sections
            .iter()
            .filter(|s| s.section_type == SectionType::Error)
            .count(),
        total_extractions: sections
            .iter()
            .filter(|s| s.section_type == SectionType::Extract)
            .count(),
        total_assertions: sections
            .iter()
            .filter(|s| s.section_type == SectionType::Asserts)
            .count(),
        has_streaming: false,
    }
}
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
