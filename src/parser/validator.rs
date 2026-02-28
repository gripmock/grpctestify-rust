// GCTF document validator - validates parsed AST
// Checks for required sections, conflicts, and data integrity

use super::ast::*;
use anyhow::{Result, bail};
use serde::Serialize;

/// Validation error
#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<usize>,
    pub severity: ErrorSeverity,
}

/// Error severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Error,
    Warning,
    #[allow(dead_code)]
    Info,
}

/// Validate a parsed GCTF document (returns all errors/warnings without bailing)
pub fn validate_document_diagnostics(document: &GctfDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check for required sections
    validate_required_sections(document, &mut errors);

    // Check for conflicts
    validate_conflicts(document, &mut errors);

    // Validate content
    validate_content(document, &mut errors);

    // Validate structure
    validate_structure(document, &mut errors);

    errors
}

/// Validate a parsed GCTF document (legacy wrapper that bails on error)
pub fn validate_document(document: &GctfDocument) -> Result<Vec<ValidationError>> {
    let errors = validate_document_diagnostics(document);
    let has_errors = errors.iter().any(|e| e.severity == ErrorSeverity::Error);

    if has_errors {
        let error_messages: Vec<String> = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .map(|e| format!("Line {}: {}", e.line.unwrap_or(0), e.message))
            .collect();

        bail!("Validation failed:\n{}", error_messages.join("\n"));
    }

    Ok(errors)
}

/// Validate required sections
fn validate_required_sections(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // ENDPOINT is required
    if document.get_endpoint().is_none() {
        errors.push(ValidationError {
            message: "ENDPOINT section is required".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }

    // ADDRESS or environment variable is required
    // The address might also be provided via CLI args or Config file, which the validator
    // doesn't have access to here. We should probably relax this check or make it a warning.
    // Ideally validation happens with full context, but for now let's check env var.
    // If neither section nor env var is present, we warn instead of error,
    // because it might be supplied at runtime.
    let env_addr = std::env::var(crate::config::ENV_GRPCTESTIFY_ADDRESS).ok();
    if document.get_address(env_addr.as_deref()).is_none() {
        // Downgrade to warning because address can come from CLI/Config
        errors.push(ValidationError {
            message: format!(
                "ADDRESS section missing (ensure {} is set or passed via --address)",
                crate::config::ENV_GRPCTESTIFY_ADDRESS
            ),
            line: None,
            severity: ErrorSeverity::Warning,
        });
    }

    // At least RESPONSE, ERROR or ASSERTS should be present for verification
    let has_response = document.first_section(SectionType::Response).is_some();
    let has_error = document.first_section(SectionType::Error).is_some();
    let has_asserts = document.first_section(SectionType::Asserts).is_some();

    if !has_response && !has_error && !has_asserts {
        errors.push(ValidationError {
            message: "At least one verification section (RESPONSE, ERROR, or ASSERTS) is required"
                .to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

/// Validate conflicts
fn validate_conflicts(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // RESPONSE and ERROR cannot both be present
    if document.has_response_error_conflict() {
        errors.push(ValidationError {
            message: "Cannot have both RESPONSE and ERROR sections".to_string(),
            line: None,
            severity: ErrorSeverity::Error,
        });
    }
}

/// Validate content
fn validate_content(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // Validate endpoint format
    if let Some(endpoint) = document.get_endpoint()
        && !endpoint.contains('/')
    {
        errors.push(ValidationError {
            message: format!(
                "Invalid endpoint format: {}. Expected format: package.Service/Method",
                endpoint
            ),
            line: document
                .first_section(SectionType::Endpoint)
                .map(|s| s.start_line),
            severity: ErrorSeverity::Error,
        });
    }

    // Validate address format
    if let Some(address) = document.get_address(None)
        && !address.contains(':')
    {
        errors.push(ValidationError {
            message: format!(
                "Invalid address format: {}. Expected format: host:port",
                address
            ),
            line: document
                .first_section(SectionType::Address)
                .map(|s| s.start_line),
            severity: ErrorSeverity::Error,
        });
    }

    // Validate JSON sections
    for section_type in [
        SectionType::Request,
        SectionType::Response,
        SectionType::Error,
    ] {
        for section in document.sections_by_type(section_type) {
            match &section.content {
                SectionContent::Json(json) => {
                    // Check if JSON is valid object or array
                    // For ERROR, we also allow strings
                    let is_valid = if section_type == SectionType::Error {
                        json.is_object() || json.is_array() || json.is_string()
                    } else {
                        json.is_object() || json.is_array()
                    };

                    if !is_valid {
                        errors.push(ValidationError {
                            message: format!(
                                "{:?} section must contain valid JSON object or array{}",
                                section_type,
                                if section_type == SectionType::Error {
                                    " or string"
                                } else {
                                    ""
                                }
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                SectionContent::JsonLines(values) => {
                    if section_type != SectionType::Response {
                        errors.push(ValidationError {
                            message: format!(
                                "{:?} section does not support newline-delimited JSON messages",
                                section_type
                            ),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    } else if values.is_empty() {
                        errors.push(ValidationError {
                            message: "RESPONSE section contains no JSON messages".to_string(),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // Validate key-value sections
    for section_type in [
        SectionType::RequestHeaders,
        SectionType::Tls,
        SectionType::Proto,
        SectionType::Options,
    ] {
        for section in document.sections_by_type(section_type) {
            if let SectionContent::KeyValues(kv) = &section.content {
                // Check for empty keys or values
                for key in kv.keys() {
                    if key.is_empty() {
                        errors.push(ValidationError {
                            message: format!("Empty key in {:?} section", section_type),
                            line: Some(section.start_line),
                            severity: ErrorSeverity::Error,
                        });
                    }
                }
            }
        }
    }

    // Validate assertions
    for section in document.sections_by_type(SectionType::Asserts) {
        if let SectionContent::Assertions(assertions) = &section.content {
            for assertion in assertions {
                if assertion.is_empty() {
                    errors.push(ValidationError {
                        message: "Empty assertion found".to_string(),
                        line: Some(section.start_line),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
        }
    }
}

/// Validate structure
fn validate_structure(document: &GctfDocument, errors: &mut Vec<ValidationError>) {
    // Check for duplicate non-multiple sections
    let mut seen_sections = std::collections::HashSet::new();

    for section in &document.sections {
        if !section.section_type.is_multiple_allowed() {
            if seen_sections.contains(&section.section_type) {
                errors.push(ValidationError {
                    message: format!("Duplicate {:?} section found", section.section_type),
                    line: Some(section.start_line),
                    severity: ErrorSeverity::Error,
                });
            }
            seen_sections.insert(section.section_type);
        }
    }

    // Validate section order (optional, but good for readability)
    // Not enforcing strict order, just checking for obvious issues
    // TODO: Add optional strict ordering validation

    // Validate inline options are only on supported sections
    for section in &document.sections {
        if !section.section_type.supports_inline_options() {
            let has_inline_options = section.inline_options.with_asserts
                || section.inline_options.partial
                || section.inline_options.tolerance.is_some()
                || !section.inline_options.redact.is_empty()
                || section.inline_options.unordered_arrays;

            if has_inline_options {
                errors.push(ValidationError {
                    message: format!(
                        "Inline options are not supported for {:?} section",
                        section.section_type
                    ),
                    line: Some(section.start_line),
                    severity: ErrorSeverity::Warning,
                });
            }
        }
    }
}

/// Check if validation passed (no errors)
#[allow(dead_code)]
pub fn validation_passed(errors: &[ValidationError]) -> bool {
    !errors.iter().any(|e| e.severity == ErrorSeverity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_document() -> GctfDocument {
        let mut doc = GctfDocument::new("test.gctf".to_string());

        doc.sections = vec![
            Section {
                section_type: SectionType::Address,
                content: SectionContent::Single("localhost:4770".to_string()),
                inline_options: InlineOptions::default(),
                raw_content: "localhost:4770".to_string(),
                start_line: 1,
                end_line: 1,
            },
            Section {
                section_type: SectionType::Endpoint,
                content: SectionContent::Single("my.Service/Method".to_string()),
                inline_options: InlineOptions::default(),
                raw_content: "my.Service/Method".to_string(),
                start_line: 3,
                end_line: 3,
            },
        ];

        doc
    }

    #[test]
    fn test_validate_required_sections_pass() {
        let doc = create_test_document();
        let result = validate_document(&doc);
        // Should fail because no REQUEST or ASSERTS
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_endpoint_format() {
        let mut doc = create_test_document();
        doc.sections[1].content = SectionContent::Single("invalid_endpoint".to_string());

        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_address_format() {
        let mut doc = create_test_document();
        doc.sections[0].content = SectionContent::Single("invalid_address".to_string());

        let result = validate_document(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_passed() {
        let errors = vec![
            ValidationError {
                message: "Warning".to_string(),
                line: Some(1),
                severity: ErrorSeverity::Warning,
            },
            ValidationError {
                message: "Info".to_string(),
                line: Some(2),
                severity: ErrorSeverity::Info,
            },
        ];

        assert!(validation_passed(&errors));
    }

    #[test]
    fn test_validation_failed() {
        let errors = vec![
            ValidationError {
                message: "Warning".to_string(),
                line: Some(1),
                severity: ErrorSeverity::Warning,
            },
            ValidationError {
                message: "Error".to_string(),
                line: Some(2),
                severity: ErrorSeverity::Error,
            },
        ];

        assert!(!validation_passed(&errors));
    }

    #[test]
    fn test_validate_document_diagnostics() {
        let doc = create_test_document();
        let errors = validate_document_diagnostics(&doc);
        // Should have some errors or warnings
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_validate_document_with_response() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and RESPONSE
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_with_error_section() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 5,
            end_line: 6,
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and ERROR
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_with_asserts() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 1".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: ".id == 1".to_string(),
            start_line: 5,
            end_line: 5,
        });

        let result = validate_document(&doc);
        // Should pass with ADDRESS, ENDPOINT, and ASSERTS
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_missing_endpoint() {
        let mut doc = create_test_document();
        doc.sections.remove(1); // Remove ENDPOINT

        let errors = validate_document_diagnostics(&doc);
        let has_endpoint_error = errors.iter().any(|e| e.message.contains("ENDPOINT"));
        assert!(has_endpoint_error);
    }

    #[test]
    fn test_validate_document_response_error_conflict() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 5,
            end_line: 6,
        });
        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(serde_json::json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"code\": 5}".to_string(),
            start_line: 7,
            end_line: 8,
        });

        let errors = validate_document_diagnostics(&doc);
        let has_conflict_error = errors
            .iter()
            .any(|e| e.message.contains("RESPONSE") && e.message.contains("ERROR"));
        assert!(has_conflict_error);
    }

    #[test]
    fn test_validate_document_empty_requests() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 5,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 6,
            end_line: 7,
        });

        let result = validate_document(&doc);
        // Empty REQUEST is allowed
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_invalid_request_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
        });

        let result = validate_document(&doc);
        // Valid JSON should pass
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_document_invalid_response_json() {
        let mut doc = create_test_document();
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(serde_json::json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"key\": \"value\"}".to_string(),
            start_line: 5,
            end_line: 6,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 7,
            end_line: 8,
        });

        let errors = validate_document_diagnostics(&doc);
        // Valid JSON should have no errors
        let has_json_errors = errors.iter().any(|e| e.message.contains("JSON"));
        assert!(!has_json_errors);
    }

    #[test]
    fn test_validate_document_address_from_env() {
        // Set env var
        unsafe {
            std::env::set_var(crate::config::ENV_GRPCTESTIFY_ADDRESS, "env:5000");
        }

        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "Service/Method".to_string(),
            start_line: 1,
            end_line: 1,
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"result\": \"ok\"}".to_string(),
            start_line: 2,
            end_line: 3,
        });

        let result = validate_document(&doc);
        // Should pass because address comes from env
        assert!(result.is_ok());

        // Clean up
        unsafe {
            std::env::remove_var(crate::config::ENV_GRPCTESTIFY_ADDRESS);
        }
    }

    #[test]
    fn test_validation_error_debug() {
        let error = ValidationError {
            message: "test error".to_string(),
            line: Some(10),
            severity: ErrorSeverity::Error,
        };
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("ValidationError"));
        assert!(debug_str.contains("test error"));
    }

    #[test]
    fn test_error_severity_serialize() {
        let error = ErrorSeverity::Error;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, "\"error\"");

        let warning = ErrorSeverity::Warning;
        let json = serde_json::to_string(&warning).unwrap();
        assert_eq!(json, "\"warning\"");
    }
}
