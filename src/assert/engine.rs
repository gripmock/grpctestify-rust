// Assertion engine using embedded jaq and legacy fallback
// Implementing a basic assertion engine that supports simple JQ-like syntax and custom functions

use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

// Plugin imports
use crate::plugins::{PluginContext, PluginManager, PluginResult};

// Jaq imports
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

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

        // 1. Try legacy engine (handles @ functions and custom operators)
        match self.evaluate_legacy(trimmed, response, headers, trailers) {
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

    // Legacy Logic (Copied and adapted)
    fn evaluate_legacy(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Result<AssertionResult> {
        let trimmed = assertion.trim();

        // Check for built-in boolean functions first (e.g. @uuid(.id))
        if trimmed.starts_with('@')
            && !trimmed.contains("==")
            && !trimmed.contains("!=")
            && !trimmed.contains('>')
            && !trimmed.contains('<')
        {
            return self.evaluate_boolean_function(trimmed, response, headers, trailers);
        }

        // List of operators sorted by length (descending)
        let operators = [
            "contains",
            "matches",
            "startsWith",
            "endsWith",
            "==",
            "!=",
            ">=",
            "<=",
            ">",
            "<",
        ];

        for op in operators {
            if let Some(idx) = trimmed.find(op) {
                let lhs_str = &trimmed[..idx].trim();
                let rhs_str = &trimmed[idx + op.len()..].trim();

                if lhs_str.is_empty() {
                    continue;
                }

                // If LHS contains pipe '|', it's likely a JQ filter, so we should skip legacy evaluation
                // unless it's inside a string (which is rare for LHS path)
                if lhs_str.contains('|') {
                    continue;
                }

                // If LHS contains '(', it might be a function call.
                // Legacy only supports functions starting with '@'.
                // If it doesn't start with '@', assume it's a JQ function (e.g. length, select, etc.)
                if lhs_str.contains('(') && !lhs_str.trim().starts_with('@') {
                    continue;
                }

                let lhs_val = self.evaluate_expression(lhs_str, response, headers, trailers);

                let rhs_val = self.parse_value(rhs_str);

                return self.compare(lhs_val, op, rhs_val, lhs_str, rhs_str);
            }
        }

        Ok(AssertionResult::Error(format!(
            "Unsupported assertion syntax: {}",
            assertion
        )))
    }

    fn evaluate_boolean_function(
        &self,
        expr: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Result<AssertionResult> {
        if let Some(start_paren) = expr.find('(') {
            if let Some(end_paren) = expr.rfind(')') {
                let func_name = &expr[0..start_paren];
                // strip @
                let plugin_name = if let Some(stripped) = func_name.strip_prefix('@') {
                    stripped
                } else {
                    func_name
                };

                let arg_str = &expr[start_paren + 1..end_paren];

                if let Some(plugin) = self.plugin_manager.get(plugin_name) {
                    let context = PluginContext {
                        response,
                        headers,
                        trailers,
                    };

                    // Special handling for @header and @trailer arguments (raw string)
                    // The legacy engine passed raw string "key" for header/trailer
                    // But other plugins evaluated the path .field

                    let args = if plugin_name == "header" || plugin_name == "trailer" {
                        vec![Value::String(arg_str.trim().trim_matches('"').to_string())]
                    } else {
                        vec![self.evaluate_expression(arg_str, response, headers, trailers)]
                    };

                    match plugin.execute(&args, &context) {
                        Ok(PluginResult::Assertion(res)) => return Ok(res),
                        Ok(PluginResult::Value(val)) => {
                            // If a plugin returns a value in a boolean context, treat truthy/falsey
                            if !val.is_null() && val != false {
                                return Ok(AssertionResult::Pass);
                            } else {
                                return Ok(AssertionResult::fail(format!(
                                    "Plugin {} returned falsy value: {:?}",
                                    plugin_name, val
                                )));
                            }
                        }
                        Err(e) => {
                            return Ok(AssertionResult::Error(format!("Plugin error: {}", e)))
                        }
                    }
                }

                return Ok(AssertionResult::Error(format!(
                    "Unknown function: {}",
                    func_name
                )));
            }
        }
        Ok(AssertionResult::Error(format!(
            "Invalid function call syntax: {}",
            expr
        )))
    }

    fn evaluate_expression(
        &self,
        expr: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Value {
        if expr.starts_with('@') {
            if let Some(start_paren) = expr.find('(') {
                if let Some(end_paren) = expr.rfind(')') {
                    let func_name = &expr[0..start_paren];
                    let plugin_name = if let Some(stripped) = func_name.strip_prefix('@') {
                        stripped
                    } else {
                        func_name
                    };

                    let arg_str = &expr[start_paren + 1..end_paren];

                    if let Some(plugin) = self.plugin_manager.get(plugin_name) {
                        let context = PluginContext {
                            response,
                            headers,
                            trailers,
                        };

                        // Recursively evaluate arguments
                        let arg_val =
                            self.evaluate_expression(arg_str, response, headers, trailers);

                        match plugin.execute(&[arg_val], &context) {
                            Ok(PluginResult::Value(v)) => return v,
                            _ => return Value::Null, // Or error? Legacy returned Null for unknowns
                        }
                    }
                }
            }
        }
        self.resolve_path(expr, response)
    }

    fn parse_value(&self, s: &str) -> Value {
        if s.starts_with('"') {
            let inner = s.trim_matches('"');
            Value::String(inner.to_string())
        } else if let Ok(_n) = s.parse::<f64>() {
            serde_json::from_str(s).unwrap_or(Value::Null)
        } else if s == "true" {
            Value::Bool(true)
        } else if s == "false" {
            Value::Bool(false)
        } else if s == "null" {
            Value::Null
        } else {
            Value::String(s.to_string())
        }
    }

    fn compare(
        &self,
        lhs: Value,
        op: &str,
        rhs: Value,
        lhs_expr: &str,
        rhs_expr: &str,
    ) -> Result<AssertionResult> {
        let pass = match op {
            "==" => lhs == rhs,
            "!=" => lhs != rhs,
            ">" => {
                if let (Some(l), Some(r)) = (lhs.as_f64(), rhs.as_f64()) {
                    l > r
                } else {
                    false
                }
            }
            "<" => {
                if let (Some(l), Some(r)) = (lhs.as_f64(), rhs.as_f64()) {
                    l < r
                } else {
                    false
                }
            }
            ">=" => {
                if let (Some(l), Some(r)) = (lhs.as_f64(), rhs.as_f64()) {
                    l >= r
                } else {
                    false
                }
            }
            "<=" => {
                if let (Some(l), Some(r)) = (lhs.as_f64(), rhs.as_f64()) {
                    l <= r
                } else {
                    false
                }
            }
            "contains" => match (&lhs, &rhs) {
                (Value::String(l), Value::String(r)) => l.contains(r),
                (Value::Array(l), r) => l.contains(r),
                (Value::Object(l), Value::String(r)) => l.contains_key(r),
                _ => false,
            },
            "startsWith" => match (&lhs, &rhs) {
                (Value::String(l), Value::String(r)) => l.starts_with(r),
                _ => false,
            },
            "endsWith" => match (&lhs, &rhs) {
                (Value::String(l), Value::String(r)) => l.ends_with(r),
                _ => false,
            },
            "matches" => match (&lhs, &rhs) {
                (Value::String(l), Value::String(r)) => {
                    if let Ok(re) = Regex::new(r) {
                        re.is_match(l)
                    } else {
                        return Ok(AssertionResult::Error(format!("Invalid regex: {}", r)));
                    }
                }
                _ => false,
            },
            _ => return Ok(AssertionResult::Error(format!("Unknown operator: {}", op))),
        };

        if pass {
            Ok(AssertionResult::Pass)
        } else {
            Ok(AssertionResult::Fail {
                message: format!(
                    "Assertion failed: {} {} {} (Values: {:?} vs {:?})",
                    lhs_expr, op, rhs_expr, lhs, rhs
                ),
                expected: Some(format!("{} {:?}", op, rhs)),
                actual: Some(format!("{:?}", lhs)),
            })
        }
    }

    fn resolve_path(&self, path: &str, root: &Value) -> Value {
        if path == "." {
            return root.clone();
        }

        let mut current = root;
        let clean_path = if let Some(stripped) = path.strip_prefix('.') {
            stripped
        } else {
            path
        };

        let mut parts = Vec::new();
        let mut start = 0;
        let chars = clean_path.chars().collect::<Vec<_>>();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '.' {
                parts.push(clean_path[start..i].to_string());
                start = i + 1;
            }
            i += 1;
        }
        parts.push(clean_path[start..].to_string());

        for part in parts {
            if part.is_empty() {
                continue;
            }

            if let Some(bracket_start) = part.find('[') {
                if let Some(bracket_end) = part.find(']') {
                    let key = &part[0..bracket_start];
                    let index_str = &part[bracket_start + 1..bracket_end];

                    if !key.is_empty() {
                        if let Some(val) = current.get(key) {
                            current = val;
                        } else {
                            return Value::Null;
                        }
                    }

                    if let Ok(idx) = index_str.parse::<usize>() {
                        if let Some(val) = current.get(idx) {
                            current = val;
                        } else {
                            return Value::Null;
                        }
                    }
                }
            } else if let Some(val) = current.get(&part) {
                current = val;
            } else {
                return Value::Null;
            }
        }

        current.clone()
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
