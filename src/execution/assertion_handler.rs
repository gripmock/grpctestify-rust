// Assertion Handler - handles assertion evaluation

use crate::assert::AssertionEngine;
use crate::parser::ast::{Section, SectionContent, SectionType};
use serde_json::Value;
use std::collections::HashMap;

/// Assertion evaluation result
#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub passed: bool,
    pub failure_messages: Vec<String>,
}

/// Assertion Handler - evaluates assertions
pub struct AssertionHandler {
    engine: AssertionEngine,
}

impl AssertionHandler {
    /// Create new assertion handler
    pub fn new(_verbose: bool) -> Self {
        Self {
            engine: AssertionEngine::new(),
        }
    }

    /// Evaluate assertions for a response
    pub fn evaluate_assertions(
        &self,
        sections: &[Section],
        target_value: &Value,
        headers: &HashMap<String, String>,
        trailers: &HashMap<String, String>,
    ) -> AssertionResult {
        let mut failure_messages = Vec::new();

        // Find ASSERTS sections and evaluate them
        for section in sections {
            if section.section_type == SectionType::Asserts
                && let SectionContent::Assertions(lines) = &section.content
            {
                let results =
                    self.engine
                        .evaluate_all(lines, target_value, Some(headers), Some(trailers));

                if self.engine.has_failures(&results) {
                    for fail in self.engine.get_failures(&results) {
                        match fail {
                            crate::assert::AssertionResult::Fail {
                                message,
                                expected,
                                actual,
                            } => {
                                let context = format!("at line {}", section.start_line);
                                failure_messages
                                    .push(format!("Assertion failed {}: {}", context, message));
                                if let (Some(exp), Some(act)) = (expected, actual) {
                                    failure_messages.push(format!(
                                        "    Expected: {}\n    Actual:   {}",
                                        exp, act
                                    ));
                                }
                            }
                            crate::assert::AssertionResult::Error(msg) => {
                                let context = format!("at line {}", section.start_line);
                                failure_messages
                                    .push(format!("Assertion error {}: {}", context, msg));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        AssertionResult {
            passed: failure_messages.is_empty(),
            failure_messages,
        }
    }

    /// Evaluate assertions for a specific section
    pub fn evaluate_section_assertions(
        &self,
        section: &Section,
        target_value: &Value,
        headers: &HashMap<String, String>,
        trailers: &HashMap<String, String>,
    ) -> AssertionResult {
        let mut failure_messages = Vec::new();

        if section.section_type == SectionType::Asserts
            && let SectionContent::Assertions(lines) = &section.content
        {
            let results =
                self.engine
                    .evaluate_all(lines, target_value, Some(headers), Some(trailers));

            if self.engine.has_failures(&results) {
                for fail in self.engine.get_failures(&results) {
                    match fail {
                        crate::assert::AssertionResult::Fail {
                            message,
                            expected,
                            actual,
                        } => {
                            let context = format!("at line {}", section.start_line);
                            failure_messages
                                .push(format!("Assertion failed {}: {}", context, message));
                            if let (Some(exp), Some(act)) = (expected, actual) {
                                failure_messages
                                    .push(format!("    Expected: {}\n    Actual:   {}", exp, act));
                            }
                        }
                        crate::assert::AssertionResult::Error(msg) => {
                            let context = format!("at line {}", section.start_line);
                            failure_messages.push(format!("Assertion error {}: {}", context, msg));
                        }
                        _ => {}
                    }
                }
            }
        }

        AssertionResult {
            passed: failure_messages.is_empty(),
            failure_messages,
        }
    }

    /// Check if section has assertions
    pub fn has_assertions(&self, section: &Section) -> bool {
        section.section_type == SectionType::Asserts
    }

    /// Get assertion lines from section
    pub fn get_assertion_lines<'a>(&self, section: &'a Section) -> Vec<&'a String> {
        if let SectionContent::Assertions(lines) = &section.content {
            lines.iter().collect()
        } else {
            Vec::new()
        }
    }

    /// Evaluate a single assertion
    pub fn evaluate_single_assertion(
        &self,
        assertion: &str,
        target_value: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Result<crate::assert::AssertionResult, String> {
        self.engine
            .evaluate(assertion, target_value, headers, trailers)
            .map_err(|e| e.to_string())
    }

    /// Evaluate assertions for a section (convenience method for runner.rs)
    pub fn evaluate_assertions_for_section(
        &self,
        lines: &[String],
        target_value: &Value,
        headers: &HashMap<String, String>,
        trailers: &HashMap<String, String>,
        context: &str,
    ) -> AssertionResult {
        let mut failure_messages = Vec::new();

        let results = self
            .engine
            .evaluate_all(lines, target_value, Some(headers), Some(trailers));

        if self.engine.has_failures(&results) {
            for fail in self.engine.get_failures(&results) {
                match fail {
                    crate::assert::AssertionResult::Fail {
                        message,
                        expected,
                        actual,
                    } => {
                        failure_messages.push(format!("Assertion failed {}: {}", context, message));
                        if let (Some(exp), Some(act)) = (expected, actual) {
                            failure_messages
                                .push(format!("    Expected: {}\n    Actual:   {}", exp, act));
                        }
                    }
                    crate::assert::AssertionResult::Error(msg) => {
                        failure_messages.push(format!("Assertion error {}: {}", context, msg));
                    }
                    _ => {}
                }
            }
        }

        AssertionResult {
            passed: failure_messages.is_empty(),
            failure_messages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_assertion_handler_new() {
        let handler = AssertionHandler::new(false);
        // Handler created successfully
        let _ = handler;
    }

    #[test]
    fn test_evaluate_assertions_pass() {
        let handler = AssertionHandler::new(false);
        let sections = vec![Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 123".to_string()]),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        }];

        let target = json!({"id": 123, "name": "test"});
        let headers = HashMap::new();
        let trailers = HashMap::new();

        let result = handler.evaluate_assertions(&sections, &target, &headers, &trailers);
        assert!(result.passed);
        assert!(result.failure_messages.is_empty());
    }

    #[test]
    fn test_evaluate_assertions_fail() {
        let handler = AssertionHandler::new(false);
        let sections = vec![Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 456".to_string()]),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        }];

        let target = json!({"id": 123, "name": "test"});
        let headers = HashMap::new();
        let trailers = HashMap::new();

        let result = handler.evaluate_assertions(&sections, &target, &headers, &trailers);
        assert!(!result.passed);
        assert!(!result.failure_messages.is_empty());
    }

    #[test]
    fn test_has_assertions() {
        let handler = AssertionHandler::new(false);
        let asserts_section = Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![]),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };

        let other_section = Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({})),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };

        assert!(handler.has_assertions(&asserts_section));
        assert!(!handler.has_assertions(&other_section));
    }

    #[test]
    fn test_get_assertion_lines() {
        let handler = AssertionHandler::new(false);
        let section = Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![
                ".id == 123".to_string(),
                ".name == \"test\"".to_string(),
            ]),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };

        let lines = handler.get_assertion_lines(&section);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_evaluate_single_assertion() {
        let handler = AssertionHandler::new(false);
        let target = json!({"id": 123, "name": "test"});
        let headers = HashMap::new();
        let trailers = HashMap::new();

        let result = handler.evaluate_single_assertion(
            ".id == 123",
            &target,
            Some(&headers),
            Some(&trailers),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_section_assertions_empty() {
        let handler = AssertionHandler::new(false);
        let section = Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![]),
            inline_options: Default::default(),
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
        };

        let target = json!({"id": 123});
        let headers = HashMap::new();
        let trailers = HashMap::new();

        let result = handler.evaluate_section_assertions(&section, &target, &headers, &trailers);
        assert!(result.passed);
    }
}
