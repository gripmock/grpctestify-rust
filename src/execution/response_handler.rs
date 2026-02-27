// Response Handler - handles response validation and processing

use crate::assert::{AssertionEngine, JsonComparator};
use crate::execution::runner::{TestExecutionResult, TestExecutionStatus};
use crate::grpc::GrpcResponse;
use crate::parser::GctfDocument;
use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionType};
use serde_json::Value;
use std::collections::HashMap;

/// Response validation result
#[derive(Debug, Clone)]
pub struct ResponseValidationResult {
    pub status: TestExecutionStatus,
    pub failure_reasons: Vec<String>,
}

/// Response Handler - validates responses against expected values
pub struct ResponseHandler {
    no_assert: bool,
    assertion_engine: AssertionEngine,
}

impl ResponseHandler {
    /// Create new response handler
    pub fn new(no_assert: bool) -> Self {
        Self {
            no_assert,
            assertion_engine: AssertionEngine::new(),
        }
    }

    /// Validate a single response message against expected value
    pub fn validate_message(
        &self,
        actual: &Value,
        expected: &Value,
        options: &InlineOptions,
    ) -> Result<(), String> {
        if self.no_assert {
            return Ok(());
        }

        let expected_clone = expected.clone();

        let diffs = JsonComparator::compare(actual, &expected_clone, options);

        if !diffs.is_empty() {
            let mut messages = Vec::new();
            for diff in diffs {
                match diff {
                    crate::assert::AssertionResult::Fail {
                        message,
                        expected: exp,
                        actual: act,
                    } => {
                        let mut msg = format!("  - {}", message);
                        if let (Some(e), Some(a)) = (exp, act) {
                            msg.push_str(&format!(
                                "\n      Expected: {}\n      Actual:   {}",
                                e, a
                            ));
                        }
                        messages.push(msg);
                    }
                    crate::assert::AssertionResult::Error(m) => {
                        messages.push(format!("  - Error: {}", m))
                    }
                    _ => {}
                }
            }
            return Err(messages.join("\n"));
        }

        Ok(())
    }

    /// Validate response with assertions attached
    pub fn validate_with_asserts(
        &self,
        response: &GrpcResponse,
        sections: &[Section],
        response_index: usize,
        variables: &HashMap<String, Value>,
    ) -> ResponseValidationResult {
        let mut failure_reasons = Vec::new();

        // Find the response section
        let mut section_count = 0;
        for (i, section) in sections.iter().enumerate() {
            if section.section_type == SectionType::Response {
                if section_count == response_index {
                    // This is our response section
                    let expected_values = Self::expected_values_for_section(section, variables);

                    // Validate each expected value against received messages
                    for (msg_idx, msg) in response.messages.iter().enumerate() {
                        if msg_idx < expected_values.len()
                            && let Err(err) = self.validate_message(
                                msg,
                                &expected_values[msg_idx],
                                &section.inline_options,
                            )
                        {
                            failure_reasons.push(format!(
                                "Response mismatch at line {}: {}",
                                section.start_line, err
                            ));
                        }
                    }

                    // Check for with_asserts option
                    if section.inline_options.with_asserts
                        && let Some(next_section) = sections.get(i + 1)
                        && next_section.section_type == SectionType::Asserts
                        && let SectionContent::Assertions(lines) = &next_section.content
                    {
                        for msg in &response.messages {
                            for assertion in lines {
                                let assert_result = self.assertion_engine.evaluate(
                                    assertion,
                                    msg,
                                    Some(&response.headers),
                                    Some(&response.trailers),
                                );
                                if let Err(err) = assert_result {
                                    failure_reasons.push(format!(
                                        "Assertion failed at line {}: {}",
                                        next_section.start_line, err
                                    ));
                                }
                            }
                        }
                    }
                    break;
                }
                section_count += 1;
            }
        }

        ResponseValidationResult {
            status: if failure_reasons.is_empty() {
                TestExecutionStatus::Pass
            } else {
                TestExecutionStatus::Fail(failure_reasons.join("\n"))
            },
            failure_reasons,
        }
    }

    /// Get expected values for a response section
    fn expected_values_for_section(
        section: &Section,
        variables: &HashMap<String, Value>,
    ) -> Vec<Value> {
        match &section.content {
            SectionContent::Json(value) => {
                let mut expected = value.clone();
                // Substitute variables in expected value
                Self::substitute_variables_in_value(&mut expected, variables);
                vec![expected]
            }
            SectionContent::JsonLines(values) => values
                .iter()
                .map(|v: &Value| {
                    let mut expected = v.clone();
                    Self::substitute_variables_in_value(&mut expected, variables);
                    expected
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Substitute variables in a JSON value (public for use by orchestrator)
    pub fn substitute_variables_in_value(value: &mut Value, variables: &HashMap<String, Value>) {
        match value {
            Value::String(s) => {
                for (var_name, var_value) in variables {
                    let pattern = format!("{{{{ {} }}}}", var_name);
                    if s.contains(&pattern) {
                        if let Value::String(replacement) = var_value {
                            *s = s.replace(&pattern, replacement);
                        } else {
                            *s = s.replace(&pattern, &var_value.to_string());
                        }
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    Self::substitute_variables_in_value(item, variables);
                }
            }
            Value::Object(map) => {
                for (_, val) in map {
                    Self::substitute_variables_in_value(val, variables);
                }
            }
            _ => {}
        }
    }

    /// Validate a full document against a response (for testing purposes)
    pub fn validate_document(
        &self,
        document: &GctfDocument,
        response: &GrpcResponse,
    ) -> TestExecutionResult {
        let mut failure_reasons: Vec<String> = Vec::new();
        let mut variables: HashMap<String, Value> = HashMap::new();

        let mut message_iter = response.messages.iter();
        let sections = &document.sections;
        let mut skip_next_section = false;
        let mut last_message: Option<Value> = None;

        for (i, section) in sections.iter().enumerate() {
            if skip_next_section {
                skip_next_section = false;
                continue;
            }

            match section.section_type {
                SectionType::Response => {
                    let expected_values = Self::expected_values_for_section(section, &variables);
                    let mut received_messages_for_section: Vec<Value> = Vec::new();

                    for expected_template in expected_values {
                        if let Some(msg) = message_iter.next() {
                            last_message = Some(msg.clone());
                            received_messages_for_section.push(msg.clone());

                            if !self.no_assert {
                                let mut expected = expected_template.clone();
                                Self::substitute_variables_in_value(&mut expected, &variables);

                                let diffs = JsonComparator::compare(
                                    msg,
                                    &expected,
                                    &section.inline_options,
                                );

                                if !diffs.is_empty() {
                                    failure_reasons.push(format!(
                                        "Response mismatch at line {}:",
                                        section.start_line
                                    ));
                                    for diff in diffs {
                                        match diff {
                                            crate::assert::AssertionResult::Fail {
                                                message,
                                                expected: exp,
                                                actual: act,
                                            } => {
                                                let mut msg = format!("  - {}", message);
                                                if let (Some(exp), Some(act)) = (exp, act) {
                                                    msg.push_str(&format!(
                                                        "\n      Expected: {}\n      Actual:   {}",
                                                        exp, act
                                                    ));
                                                }
                                                failure_reasons.push(msg);
                                            }
                                            crate::assert::AssertionResult::Error(m) => {
                                                failure_reasons.push(format!("  - Error: {}", m))
                                            }
                                            _ => {}
                                        }
                                    }

                                    failure_reasons
                                        .push(crate::assert::get_json_diff(&expected, msg));
                                }
                            }
                        } else if !self.no_assert {
                            failure_reasons.push(format!(
                                "Expected message for RESPONSE section at line {}, but no more messages received",
                                section.start_line
                            ));
                            break;
                        }
                    }

                    if section.inline_options.with_asserts
                        && let Some(next_section) = sections.get(i + 1)
                        && next_section.section_type == SectionType::Asserts
                        && !self.no_assert
                        && let SectionContent::Assertions(lines) = &next_section.content
                    {
                        for msg in &received_messages_for_section {
                            let result = self.assertion_engine.evaluate_all(
                                lines,
                                msg,
                                Some(&response.headers),
                                Some(&response.trailers),
                            );

                            if self.assertion_engine.has_failures(&result) {
                                for fail in self.assertion_engine.get_failures(&result) {
                                    match fail {
                                        crate::assert::AssertionResult::Fail {
                                            message,
                                            expected,
                                            actual,
                                        } => {
                                            let ctx = format!(
                                                "(attached to RESPONSE at line {})",
                                                section.start_line
                                            );
                                            failure_reasons.push(format!(
                                                "Assertion failed {}: {}",
                                                ctx, message
                                            ));
                                            if let (Some(exp), Some(act)) = (expected, actual) {
                                                failure_reasons.push(format!(
                                                    "    Expected: {}\n    Actual:   {}",
                                                    exp, act
                                                ));
                                            }
                                        }
                                        crate::assert::AssertionResult::Error(m) => {
                                            let ctx = format!(
                                                "(attached to RESPONSE at line {})",
                                                section.start_line
                                            );
                                            failure_reasons
                                                .push(format!("Assertion error {}: {}", ctx, m));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        skip_next_section = true;
                    }
                }
                SectionType::Asserts => {
                    if let Some(msg) = message_iter.next() {
                        last_message = Some(msg.clone());
                        if !self.no_assert
                            && let SectionContent::Assertions(lines) = &section.content
                        {
                            let result = self.assertion_engine.evaluate_all(
                                lines,
                                msg,
                                Some(&response.headers),
                                Some(&response.trailers),
                            );

                            if self.assertion_engine.has_failures(&result) {
                                for fail in self.assertion_engine.get_failures(&result) {
                                    match fail {
                                        crate::assert::AssertionResult::Fail {
                                            message,
                                            expected,
                                            actual,
                                        } => {
                                            let ctx = format!("at line {}", section.start_line);
                                            failure_reasons.push(format!(
                                                "Assertion failed {}: {}",
                                                ctx, message
                                            ));
                                            if let (Some(exp), Some(act)) = (expected, actual) {
                                                failure_reasons.push(format!(
                                                    "    Expected: {}\n    Actual:   {}",
                                                    exp, act
                                                ));
                                            }
                                        }
                                        crate::assert::AssertionResult::Error(m) => {
                                            let ctx = format!("at line {}", section.start_line);
                                            failure_reasons
                                                .push(format!("Assertion error {}: {}", ctx, m));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    } else if !self.no_assert {
                        failure_reasons.push(format!(
                            "Expected message for ASSERTS section at line {}, but no more messages received",
                            section.start_line
                        ));
                    }
                }
                SectionType::Extract => {
                    if let Some(msg) = &last_message {
                        if let SectionContent::Extract(extractions) = &section.content {
                            for (key, query) in extractions {
                                match self.assertion_engine.query(query, msg) {
                                    Ok(results) => {
                                        if let Some(val) = results.first() {
                                            variables.insert(key.clone(), val.clone());
                                        } else {
                                            failure_reasons.push(format!(
                                                 "Extraction failed at line {}: Query '{}' returned no results",
                                                 section.start_line, query
                                             ));
                                        }
                                    }
                                    Err(e) => {
                                        failure_reasons.push(format!(
                                            "Extraction error at line {}: {}",
                                            section.start_line, e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        failure_reasons.push(format!(
                            "EXTRACT at line {} requires a previous response message",
                            section.start_line
                        ));
                    }
                }
                _ => {}
            }
        }

        if !failure_reasons.is_empty() {
            TestExecutionResult::fail(
                format!("Validation failed:\n  - {}", failure_reasons.join("\n  - ")),
                None,
            )
        } else {
            TestExecutionResult::pass(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_response_handler_new() {
        let handler = ResponseHandler::new(false);
        assert!(!handler.no_assert);
    }

    #[test]
    fn test_validate_message_exact_match() {
        let handler = ResponseHandler::new(false);
        let actual = json!({"id": 123, "name": "test"});
        let expected = json!({"id": 123, "name": "test"});
        let options = InlineOptions::default();

        let result = handler.validate_message(&actual, &expected, &options);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_message_mismatch() {
        let handler = ResponseHandler::new(false);
        let actual = json!({"id": 123, "name": "test"});
        let expected = json!({"id": 456, "name": "test"});
        let options = InlineOptions::default();

        let result = handler.validate_message(&actual, &expected, &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_message_partial_match() {
        let handler = ResponseHandler::new(false);
        let actual = json!({"id": 123, "name": "test", "extra": "field"});
        let expected = json!({"id": 123});
        let mut options = InlineOptions::default();
        options.partial = true;

        let result = handler.validate_message(&actual, &expected, &options);
        assert!(result.is_ok());
    }

    #[test]
    fn test_substitute_variables_in_value() {
        let mut value = json!({"id": "{{ user_id }}", "name": "test"});
        let mut variables = HashMap::new();
        variables.insert("user_id".to_string(), json!("123"));

        ResponseHandler::substitute_variables_in_value(&mut value, &variables);

        assert_eq!(value["id"], "123");
        assert_eq!(value["name"], "test");
    }

    #[test]
    fn test_response_handler_no_assert() {
        let handler = ResponseHandler::new(true);
        let actual = json!({"id": 123});
        let expected = json!({"id": 456});
        let options = InlineOptions::default();

        // Should always pass when no_assert is true
        let result = handler.validate_message(&actual, &expected, &options);
        assert!(result.is_ok());
    }
}
