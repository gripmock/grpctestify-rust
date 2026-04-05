// Assertion engine using embedded jaq and operators fallback

use anyhow::Result;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Plugin imports
use crate::plugins::AssertionTiming;
use crate::plugins::PluginManager;

// Jaq imports
use jaq_core::{Compiler, Ctx, Vars, data, load, unwrap_valr};
use jaq_json::{Map as JaqMap, Num as JaqNum, Rc as JaqRc, Val as JaqVal};

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

type JaqFilter = jaq_core::Filter<data::JustLut<JaqVal>>;

thread_local! {
    static JAQ_FILTER_CACHE: RefCell<HashMap<String, Rc<JaqFilter>>> = RefCell::new(HashMap::new());
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
        self.evaluate_with_timing(assertion, response, headers, trailers, None)
    }

    pub fn evaluate_with_timing(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
    ) -> Result<AssertionResult> {
        let trimmed = assertion.trim();

        // 1. Try operators engine (handles @ functions and custom operators)
        match operators::evaluate_assertion(
            &self.plugin_manager,
            trimmed,
            response,
            headers,
            trailers,
            timing,
        ) {
            Ok(AssertionResult::Error(msg)) if msg.starts_with("Unsupported assertion syntax") => {
                // Fallback to JQ
                self.evaluate_jaq(trimmed, response)
            }
            other => other,
        }
    }

    /// Execute a JQ query and return the result(s)
    pub fn query(&self, expr: &str, input: &Value) -> Result<Vec<Value>> {
        let values = self.run_jaq(expr, input)?;
        Ok(values.iter().map(jaq_to_json).collect())
    }

    fn evaluate_jaq(&self, expr: &str, response: &Value) -> Result<AssertionResult> {
        let out = match self.run_jaq(expr, response) {
            Ok(out) => out,
            Err(e) => return Ok(AssertionResult::Error(format!("JQ Parse Error: {}", e))),
        };

        let mut passed = false;
        let mut seen_false = false;

        for val in out {
            if matches!(val, JaqVal::Bool(true)) {
                passed = true;
            } else {
                seen_false = true;
            }
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

    fn run_jaq(&self, expr: &str, input: &Value) -> Result<Vec<JaqVal>> {
        let filter = Self::get_or_compile_jaq_filter(expr)?;

        let input = json_to_jaq(input);

        let ctx = Ctx::<data::JustLut<JaqVal>>::new(&filter.lut, Vars::new([]));
        let out = filter.id.run((ctx, input)).map(unwrap_valr);

        let mut values = Vec::new();
        for item in out {
            match item {
                Ok(v) => values.push(v),
                Err(e) => return Err(anyhow::anyhow!("JQ Runtime Error: {}", e)),
            }
        }

        Ok(values)
    }

    fn get_or_compile_jaq_filter(expr: &str) -> Result<Rc<JaqFilter>> {
        use jaq_core::defs as core_defs;
        use jaq_core::funs as core_funs;

        if let Some(cached) = JAQ_FILTER_CACHE.with(|cache| cache.borrow().get(expr).cloned()) {
            return Ok(cached);
        }

        let arena = load::Arena::default();
        let defs = core_defs().chain(jaq_std::defs()).chain(jaq_json::defs());
        let funs = core_funs().chain(jaq_std::funs()).chain(jaq_json::funs());
        let loader = load::Loader::new(defs);
        let program = load::File {
            code: expr,
            path: (),
        };

        let modules = loader
            .load(&arena, program)
            .map_err(|errs| anyhow::anyhow!("Failed to parse JQ expression: {:?}", errs))?;

        let filter = Compiler::default()
            .with_funs(funs)
            .compile(modules)
            .map_err(|errs| anyhow::anyhow!("Failed to compile JQ expression: {:?}", errs))?;

        let filter = Rc::new(filter);
        JAQ_FILTER_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .insert(expr.to_string(), Rc::clone(&filter));
        });

        Ok(filter)
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
        self.evaluate_all_with_timing(assertions, response, headers, trailers, None)
    }

    pub fn evaluate_all_with_timing(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
    ) -> Vec<AssertionResult> {
        assertions
            .iter()
            .map(|assertion| {
                self.evaluate_with_timing(assertion, response, headers, trailers, timing)
                    .unwrap_or_else(|e| AssertionResult::Error(format!("Internal error: {}", e)))
            })
            .collect()
    }
}

fn json_to_jaq(value: &Value) -> JaqVal {
    match value {
        Value::Null => JaqVal::Null,
        Value::Bool(v) => JaqVal::Bool(*v),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JaqVal::Num(JaqNum::from_integral(i))
            } else if let Some(u) = n.as_u64() {
                JaqVal::Num(JaqNum::from_integral(u))
            } else if let Some(f) = n.as_f64() {
                JaqVal::Num(JaqNum::Float(f))
            } else {
                JaqVal::Null
            }
        }
        Value::String(s) => JaqVal::utf8_str(s.clone()),
        Value::Array(items) => JaqVal::Arr(JaqRc::new(items.iter().map(json_to_jaq).collect())),
        Value::Object(obj) => {
            let map: JaqMap = obj
                .iter()
                .map(|(k, v)| (JaqVal::utf8_str(k.clone()), json_to_jaq(v)))
                .collect();
            JaqVal::Obj(JaqRc::new(map))
        }
    }
}

fn jaq_to_json(value: &JaqVal) -> Value {
    match value {
        JaqVal::Null => Value::Null,
        JaqVal::Bool(v) => Value::Bool(*v),
        JaqVal::Num(n) => match n {
            JaqNum::Int(v) => Value::Number(serde_json::Number::from(*v)),
            JaqNum::Float(v) => serde_json::Number::from_f64(*v)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            JaqNum::BigInt(bi) => {
                // Try to fit in isize first (public API), then fall back to string parse
                if let Some(i) = n.as_isize() {
                    Value::Number(serde_json::Number::from(i))
                } else {
                    // BigInt too large for isize — avoid JSON parser on hot path
                    let s = bi.to_string();
                    if let Ok(i) = s.parse::<i64>() {
                        Value::Number(serde_json::Number::from(i))
                    } else if let Ok(u) = s.parse::<u64>() {
                        Value::Number(serde_json::Number::from(u))
                    } else {
                        Value::Null
                    }
                }
            }
            JaqNum::Dec(s) => {
                // Dec is a string like "3.14" — parse as f64 directly, no JSON parser
                s.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }
        },
        JaqVal::TStr(s) | JaqVal::BStr(s) => {
            match std::str::from_utf8(s.as_ref()) {
                Ok(v) => Value::String(v.to_string()),
                Err(_) => Value::Null, // non-UTF8 bytes can't be represented in JSON
            }
        }
        JaqVal::Arr(items) => Value::Array(items.iter().map(jaq_to_json).collect()),
        JaqVal::Obj(obj) => {
            let map: serde_json::Map<String, Value> = obj
                .iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        JaqVal::TStr(s) | JaqVal::BStr(s) => {
                            std::str::from_utf8(s.as_ref()).ok().map(str::to_owned)
                        }
                        _ => None,
                    }?;
                    Some((key, jaq_to_json(v)))
                })
                .collect();
            Value::Object(map)
        }
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
    fn test_evaluate_empty_plugin() {
        let engine = AssertionEngine::new();
        let response = json!({"tags": []});

        let result = engine
            .evaluate("@empty(.tags)", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
            // Pass
        } else {
            panic!("Expected Pass for empty value");
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

    #[test]
    fn test_jaq_to_json_dec_number() {
        let dec = JaqVal::Num(JaqNum::Dec(JaqRc::new("2.5".to_string())));
        assert_eq!(jaq_to_json(&dec), json!(2.5));
    }

    #[test]
    fn test_jaq_to_json_invalid_dec_number() {
        let dec = JaqVal::Num(JaqNum::Dec(JaqRc::new("not-a-number".to_string())));
        assert_eq!(jaq_to_json(&dec), Value::Null);
    }
}
