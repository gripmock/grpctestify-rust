// Test validation logic

use crate::execution::TestExecutionResult;
use crate::parser::ast::GctfDocument;

/// Test validator
pub struct TestValidator;

impl TestValidator {
    /// Validate test document before execution
    pub fn validate(document: &GctfDocument) -> Result<(), String> {
        // Check for required sections
        Self::validate_required_sections(document)?;

        // Check for conflicting sections
        Self::validate_no_conflicts(document)?;

        // Validate EXTRACT section
        Self::validate_extract(document)?;

        // Validate ASSERTS section
        Self::validate_asserts(document)?;

        Ok(())
    }

    /// Validate required sections exist
    fn validate_required_sections(document: &GctfDocument) -> Result<(), String> {
        let has_endpoint = document
            .sections
            .iter()
            .any(|s| matches!(s.section_type, crate::parser::ast::SectionType::Endpoint));

        if !has_endpoint {
            return Err("Missing required ENDPOINT section".to_string());
        }

        let has_request = document
            .sections
            .iter()
            .any(|s| matches!(s.section_type, crate::parser::ast::SectionType::Request));

        if !has_request {
            return Err("Missing required REQUEST section".to_string());
        }

        Ok(())
    }

    /// Validate no conflicting sections
    fn validate_no_conflicts(document: &GctfDocument) -> Result<(), String> {
        let has_response = document
            .sections
            .iter()
            .any(|s| matches!(s.section_type, crate::parser::ast::SectionType::Response));

        let has_error = document
            .sections
            .iter()
            .any(|s| matches!(s.section_type, crate::parser::ast::SectionType::Error));

        if has_response && has_error {
            return Err("Cannot have both RESPONSE and ERROR sections".to_string());
        }

        Ok(())
    }

    /// Validate EXTRACT section
    fn validate_extract(document: &GctfDocument) -> Result<(), String> {
        let extract_sections: Vec<_> = document
            .sections
            .iter()
            .filter(|s| matches!(s.section_type, crate::parser::ast::SectionType::Extract))
            .collect();

        if extract_sections.len() > 1 {
            return Err("Multiple EXTRACT sections found".to_string());
        }

        Ok(())
    }

    /// Validate ASSERTS section
    fn validate_asserts(document: &GctfDocument) -> Result<(), String> {
        let asserts_sections: Vec<_> = document
            .sections
            .iter()
            .filter(|s| matches!(s.section_type, crate::parser::ast::SectionType::Asserts))
            .collect();

        if asserts_sections.len() > 1 {
            return Err("Multiple ASSERTS sections found".to_string());
        }

        Ok(())
    }

    /// Validate test result after execution
    pub fn validate_result(result: &TestExecutionResult) -> bool {
        matches!(result.status, crate::execution::TestExecutionStatus::Pass)
    }

    /// Get result summary
    pub fn get_result_summary(result: &TestExecutionResult) -> String {
        let duration = result.grpc_duration_ms.unwrap_or(0);
        match &result.status {
            crate::execution::TestExecutionStatus::Pass => {
                format!("✓ Passed ({}ms)", duration)
            }
            crate::execution::TestExecutionStatus::Fail(msg) => {
                format!("✗ Failed: {} ({}ms)", msg, duration)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{GctfDocument, Section, SectionContent, SectionType};
    use serde_json::json;

    fn create_test_document() -> GctfDocument {
        GctfDocument {
            file_path: "test.gctf".to_string(),
            sections: vec![
                Section {
                    section_type: SectionType::Endpoint,
                    content: SectionContent::Single("test.Service/Method".to_string()),
                    inline_options: Default::default(),
                    raw_content: "test.Service/Method".to_string(),
                    start_line: 1,
                    end_line: 2,
                },
                Section {
                    section_type: SectionType::Request,
                    content: SectionContent::Json(json!({"id": 123})),
                    inline_options: Default::default(),
                    raw_content: r#"{"id": 123}"#.to_string(),
                    start_line: 3,
                    end_line: 5,
                },
            ],
            metadata: crate::parser::ast::DocumentMetadata {
                source: None,
                mtime: None,
                parsed_at: 0,
            },
        }
    }

    #[test]
    fn test_validate_required_sections() {
        let doc = create_test_document();
        let result = TestValidator::validate(&doc);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_missing_endpoint() {
        let mut doc = create_test_document();
        doc.sections
            .retain(|s| !matches!(s.section_type, SectionType::Endpoint));

        let result = TestValidator::validate(&doc);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ENDPOINT"));
    }

    #[test]
    fn test_validate_missing_request() {
        let mut doc = create_test_document();
        doc.sections
            .retain(|s| !matches!(s.section_type, SectionType::Request));

        let result = TestValidator::validate(&doc);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("REQUEST"));
    }

    #[test]
    fn test_validate_no_conflicts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "ok"})),
            inline_options: Default::default(),
            raw_content: r#"{"result": "ok"}"#.to_string(),
            start_line: 6,
            end_line: 8,
        });
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(json!({"code": 5})),
            inline_options: Default::default(),
            raw_content: r#"{"code": 5}"#.to_string(),
            start_line: 9,
            end_line: 11,
        });

        let result = TestValidator::validate(&doc);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("RESPONSE"));
    }

    #[test]
    fn test_validate_result_pass() {
        let result = TestExecutionResult::pass(Some(100));
        assert!(TestValidator::validate_result(&result));
    }

    #[test]
    fn test_validate_result_fail() {
        let result = TestExecutionResult::fail("Assertion failed".to_string(), Some(100));
        assert!(!TestValidator::validate_result(&result));
    }

    #[test]
    fn test_get_result_summary_pass() {
        let result = TestExecutionResult::pass(Some(100));
        let summary = TestValidator::get_result_summary(&result);
        assert!(summary.contains("Passed"));
        assert!(summary.contains("100ms"));
    }

    #[test]
    fn test_get_result_summary_fail() {
        let result = TestExecutionResult::fail("Error".to_string(), Some(100));
        let summary = TestValidator::get_result_summary(&result);
        assert!(summary.contains("Failed"));
    }
}
