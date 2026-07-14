#[cfg(test)]
use crate::model::{TestExecutionResult, TestExecutionStatus};
#[cfg(test)]
use apif_ast::GctfDocument;

#[cfg(test)]
pub struct TestValidator;
#[cfg(test)]
impl TestValidator {
    pub fn validate(document: &GctfDocument) -> Result<(), String> {
        if document
            .first_section(apif_ast::SectionType::Request)
            .is_none()
        {
            return Err("No REQUEST section".into());
        }
        if document
            .first_section(apif_ast::SectionType::Endpoint)
            .is_none()
        {
            return Err("No ENDPOINT section".into());
        }
        Ok(())
    }
    pub fn validate_result(result: &TestExecutionResult) -> bool {
        matches!(result.status, TestExecutionStatus::Pass)
    }
    pub fn get_result_summary(result: &TestExecutionResult) -> String {
        match &result.status {
            TestExecutionStatus::Pass => format!(
                "✓ {} passed",
                result
                    .call_duration_ms
                    .map(|d| format!("{}ms", d))
                    .unwrap_or_default()
            ),
            TestExecutionStatus::Fail(msg) => format!("✗ FAILED: {}", msg),
        }
    }
}
