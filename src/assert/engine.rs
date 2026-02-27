// Assertion engine using embedded jaq and operators fallback

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

// Plugin imports
use crate::plugins::PluginManager;

// Jaq imports
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

// Operators module
use super::operators;

/// Assertion result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionResult {
    Pass,
    Fail {
        message: String,
        expected: Option<String>,
        actual: Option<String>,
    },
    Error(String),
}

impl AssertionResult {
    pub fn fail(message: impl Into<String>) -> Self {
        Self::Fail {
            message: message.into(),
            expected: None,
            actual: None,
        }
    }

    pub fn fail_with_diff(
        message: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::Fail {
            message: message.into(),
            expected: Some(expected.into()),
            actual: Some(actual.into()),
        }
    }
}

/// Assertion engine
pub struct AssertionEngine {
    plugin_manager: PluginManager,
}

impl AssertionEngine {
    /// Create a new assertion engine
    pub fn new() -> Self {
        Self {
            plugin_manager: PluginManager::new(),
        }
    }

    /// Evaluate a single assertion
    pub fn evaluate(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Result<AssertionResult> {
        let trimmed = assertion.trim();

        // 1. Try operators engine (handles @ functions and custom operators)
        match operators::evaluate_legacy(&self.plugin_manager, trimmed, response, headers, trailers)
        {
            Ok(AssertionResult::Error(msg)) if msg.starts_with("Unsupported assertion syntax") => {
                // Fallback to JQ
                self.evaluate_jaq(trimmed, response)
            }
            other => other,
        }
    }

    /// Execute a JQ query and return the result(s)
    pub fn query(&self, expr: &str, input: &Value) -> Result<Vec<Value>> {
        // Parse using jaq_parse::parse
        let main_expr = self.parse_jaq(expr)?;

        // Compile
        let mut defs = ParseCtx::new(Vec::new());
        defs.insert_natives(jaq_core::core());
        defs.insert_defs(jaq_std::std());

        let filter = defs.compile(main_expr);

        // Execute
        let inputs = RcIter::new(core::iter::empty());
        let out = filter.run((Ctx::new(vec![], &inputs), Val::from(input.clone())));

        let mut results = Vec::new();
        for r in out {
            match r {
                Ok(val) => {
                    // Convert Val back to serde_json::Value
                    results.push(val.into());
                }
                Err(e) => return Err(anyhow::anyhow!("JQ Runtime Error: {}", e)),
            }
        }

        Ok(results)
    }

    fn evaluate_jaq(&self, expr: &str, response: &Value) -> Result<AssertionResult> {
        // Parse using jaq_parse::parse
        let main_expr = match self.parse_jaq(expr) {
            Ok(main) => main,
            Err(e) => return Ok(AssertionResult::Error(format!("JQ Parse Error: {}", e))),
        };

        // Compile
        let mut defs = ParseCtx::new(Vec::new());
        defs.insert_natives(jaq_core::core());
        defs.insert_defs(jaq_std::std());

        let filter = defs.compile(main_expr);

        // Execute
        let inputs = RcIter::new(core::iter::empty());
        let out = filter.run((Ctx::new(vec![], &inputs), Val::from(response.clone())));

        let mut passed = false;
        let mut seen_false = false;
        let mut errors = Vec::new();

        for r in out {
            match r {
                Ok(val) => {
                    if val.as_bool() {
                        passed = true;
                    } else {
                        seen_false = true;
                    }
                }
                Err(e) => errors.push(format!("{}", e)),
            }
        }

        if !errors.is_empty() {
            return Ok(AssertionResult::Error(format!(
                "JQ Runtime Error: {}",
                errors.join(", ")
            )));
        }

        if seen_false {
            return Ok(AssertionResult::fail(format!(
                "JQ assertion evaluated to false: {}",
                expr
            )));
        }

        if passed {
            Ok(AssertionResult::Pass)
        } else {
            Ok(AssertionResult::fail(format!(
                "JQ assertion produced no output (falsey): {}",
                expr
            )))
        }
    }

    // Helper to parse JQ expression using jaq_parse
    fn parse_jaq(&self, expr: &str) -> Result<jaq_syn::Main> {
        let parser = jaq_parse::main();

        // Use jaq_parse::parse
        let result = jaq_parse::parse(expr, parser);

        // Result is (Option<Main>, Vec<Error>)
        match result.0 {
            Some(main) => Ok(main),
            None => {
                let errs = result.1;
                Err(anyhow::anyhow!("Failed to parse JQ expression: {:?}", errs))
            }
        }
    }

    // Check if any assertion failed (re-exported wrapper)
    pub fn has_failures(&self, results: &[AssertionResult]) -> bool {
        results
            .iter()
            .any(|r| matches!(r, AssertionResult::Fail { .. } | AssertionResult::Error(_)))
    }

    // Get failed assertions (re-exported wrapper)
    pub fn get_failures<'a>(&self, results: &'a [AssertionResult]) -> Vec<&'a AssertionResult> {
        results
            .iter()
            .filter(|r| matches!(r, AssertionResult::Fail { .. } | AssertionResult::Error(_)))
            .collect()
    }

    // Evaluate multiple assertions (re-exported wrapper)
    pub fn evaluate_all(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Vec<AssertionResult> {
        assertions
            .iter()
            .map(|assertion| {
                self.evaluate(assertion, response, headers, trailers)
                    .unwrap_or_else(|e| AssertionResult::Error(format!("Internal error: {}", e)))
            })
            .collect()
    }
}

impl Default for AssertionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_response() -> Value {
        json!({
            "id": 123,
            "name": "test",
            "email": "test@example.com",
            "active": true,
            "tags": ["a", "b", "c"],
            "nested": {
                "value": 42
            }
        })
    }

    #[test]
    fn test_assertion_engine_new() {
        let engine = AssertionEngine::new();
        // Should have default plugins registered
        assert!(engine.plugin_manager.get("uuid").is_some());
        assert!(engine.plugin_manager.get("email").is_some());
    }

    #[test]
    fn test_assertion_engine_default() {
        let engine = AssertionEngine::default();
        assert!(engine.plugin_manager.get("uuid").is_some());
    }

    #[test]
    fn test_assertion_result_fail() {
        let result = AssertionResult::fail("test message");
        if let AssertionResult::Fail { message, .. } = result {
            assert_eq!(message, "test message");
        } else {
            panic!("Expected Fail result");
        }
    }

    #[test]
    fn test_assertion_result_fail_with_diff() {
        let result = AssertionResult::fail_with_diff("mismatch", "expected", "actual");
        if let AssertionResult::Fail {
            message,
            expected,
            actual,
        } = result
        {
            assert_eq!(message, "mismatch");
            assert_eq!(expected, Some("expected".to_string()));
            assert_eq!(actual, Some("actual".to_string()));
        } else {
            panic!("Expected Fail result");
        }
    }

    #[test]
    fn test_assertion_result_debug() {
        let result = AssertionResult::Pass;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Pass"));
    }

    #[test]
    fn test_evaluate_plugin_function() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        // Test @email plugin
        let result = engine
            .evaluate("@email(.email)", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for valid email");
        }
    }

    #[test]
    fn test_evaluate_plugin_function_invalid() {
        let engine = AssertionEngine::new();
        let response = json!({"email": "not-an-email"});

        let result = engine
            .evaluate("@email(.email)", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { .. } = result {
            // Pass
        } else {
            panic!("Expected Fail for invalid email");
        }
    }

    #[test]
    fn test_evaluate_equality_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id == 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for equality check");
        }
    }

    #[test]
    fn test_evaluate_equality_operator_fail() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id == 456", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { .. } = result {
            // Pass
        } else {
            panic!("Expected Fail for equality check");
        }
    }

    #[test]
    fn test_evaluate_inequality_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id != 456", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for inequality check");
        }
    }

    #[test]
    fn test_evaluate_contains_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name contains \"test\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for contains check");
        }
    }

    #[test]
    fn test_evaluate_contains_operator_array() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags contains \"a\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for array contains check");
        }
    }

    #[test]
    fn test_evaluate_starts_with_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name startsWith \"te\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for startsWith check");
        }
    }

    #[test]
    fn test_evaluate_ends_with_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name endsWith \"st\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for endsWith check");
        }
    }

    #[test]
    fn test_evaluate_numeric_greater_than() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine.evaluate(".id > 100", &response, None, None).unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for greater than check");
        }
    }

    #[test]
    fn test_evaluate_numeric_less_than() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine.evaluate(".id < 200", &response, None, None).unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for less than check");
        }
    }

    #[test]
    fn test_evaluate_numeric_gte() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id >= 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for gte check");
        }
    }

    #[test]
    fn test_evaluate_numeric_lte() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id <= 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for lte check");
        }
    }

    #[test]
    fn test_evaluate_matches_regex() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name matches \"^te.*t$\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for regex match");
        }
    }

    #[test]
    fn test_evaluate_matches_regex_fail() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name matches \"^xyz\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { .. } = result {
            // Pass
        } else {
            panic!("Expected Fail for regex match");
        }
    }

    #[test]
    fn test_evaluate_header_plugin() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let result = engine
            .evaluate("@header(\"content-type\")", &response, Some(&headers), None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for header check");
        }
    }

    #[test]
    fn test_evaluate_trailer_plugin() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let mut trailers = HashMap::new();
        trailers.insert("x-custom".to_string(), "value".to_string());

        let result = engine
            .evaluate("@trailer(\"x-custom\")", &response, None, Some(&trailers))
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for trailer check");
        }
    }

    #[test]
    fn test_evaluate_nested_path() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".nested.value == 42", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for nested path check");
        }
    }

    #[test]
    fn test_evaluate_boolean_path() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".active == true", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for boolean check");
        }
    }

    #[test]
    fn test_evaluate_array_index() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags[0] == \"a\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for array index check");
        }
    }

    #[test]
    fn test_evaluate_unsupported_syntax() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        // This should fall through to JQ evaluation
        let result = engine.evaluate("some_unknown_function()", &response, None, None);
        // Should not panic, should return Error or handle gracefully
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_all() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let assertions = vec![".id == 123".to_string(), ".name == \"test\"".to_string()];

        let results = engine.evaluate_all(&assertions, &response, None, None);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| matches!(r, AssertionResult::Pass)));
    }

    #[test]
    fn test_evaluate_all_with_failure() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let assertions = vec![".id == 123".to_string(), ".id == 999".to_string()];

        let results = engine.evaluate_all(&assertions, &response, None, None);
        assert_eq!(results.len(), 2);
        assert!(matches!(&results[0], AssertionResult::Pass));
        assert!(matches!(&results[1], AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_query_jq_simple() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".id", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(123));
    }

    #[test]
    fn test_query_jq_nested() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".nested.value", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(42));
    }

    #[test]
    fn test_query_jq_array() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".tags[]", &response).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], json!("a"));
        assert_eq!(results[1], json!("b"));
        assert_eq!(results[2], json!("c"));
    }

    #[test]
    fn test_query_jq_filter() {
        let engine = AssertionEngine::new();
        let response = json!([1, 2, 3, 4, 5]);

        let results = engine.query(".[] | select(. > 3)", &response).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], json!(4));
        assert_eq!(results[1], json!(5));
    }

    #[test]
    fn test_query_jq_length() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".tags | length", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(3));
    }

    #[test]
    fn test_query_invalid_expression() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query("invalid[[[", &response);
        assert!(results.is_err());
    }
}
