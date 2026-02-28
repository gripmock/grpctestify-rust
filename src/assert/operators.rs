// Operators assertion engine
// Supports @plugin functions and custom operators (==, !=, contains, etc.)

use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

use crate::assert::engine::AssertionResult;
use crate::plugins::{PluginContext, PluginManager, PluginResult, normalize_plugin_name};

/// Evaluate assertion expression (plugins and operators)
pub fn evaluate_assertion(
    plugin_manager: &PluginManager,
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
        return evaluate_boolean_function(plugin_manager, trimmed, response, headers, trailers);
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

            // If LHS contains pipe '|', it's likely a JQ filter
            if lhs_str.contains('|') {
                continue;
            }

            // If LHS contains '(', it might be a function call.
            // Legacy only supports functions starting with '@'.
            if lhs_str.contains('(') && !lhs_str.trim().starts_with('@') {
                continue;
            }

            let lhs_val = evaluate_expression(plugin_manager, lhs_str, response, headers, trailers);
            let rhs_val = parse_value(rhs_str);

            return compare(lhs_val, op, rhs_val, lhs_str, rhs_str);
        }
    }

    Ok(AssertionResult::Error(format!(
        "Unsupported assertion syntax: {}",
        assertion
    )))
}

fn evaluate_boolean_function(
    plugin_manager: &PluginManager,
    expr: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
) -> Result<AssertionResult> {
    if let (Some(start_paren), Some(end_paren)) = (expr.find('('), expr.rfind(')')) {
        let func_name = &expr[0..start_paren];
        let arg_str = &expr[start_paren + 1..end_paren];

        let resolved_name = normalize_plugin_name(func_name);

        if let Some(plugin) = plugin_manager.get(resolved_name) {
            let context = PluginContext {
                response,
                headers,
                trailers,
            };

            let args = parse_plugin_arguments(plugin_manager, arg_str, response, headers, trailers);

            return match plugin.execute(&args, &context) {
                Ok(PluginResult::Assertion(res)) => Ok(res),
                Ok(PluginResult::Value(val)) => {
                    if !val.is_null() && val != false {
                        Ok(AssertionResult::Pass)
                    } else {
                        Ok(AssertionResult::fail(format!(
                            "Plugin {} returned falsy value: {:?}",
                            resolved_name, val
                        )))
                    }
                }
                Err(e) => Ok(AssertionResult::Error(format!("Plugin error: {}", e))),
            };
        }
    }
    Ok(AssertionResult::Error(format!(
        "Invalid function call syntax: {}",
        expr
    )))
}

fn evaluate_expression(
    plugin_manager: &PluginManager,
    expr: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
) -> Value {
    if expr.starts_with('@')
        && let (Some(start_paren), Some(end_paren)) = (expr.find('('), expr.rfind(')'))
    {
        let func_name = &expr[0..start_paren];
        let arg_str = &expr[start_paren + 1..end_paren];

        let resolved_name = normalize_plugin_name(func_name);

        if let Some(plugin) = plugin_manager.get(resolved_name) {
            let context = PluginContext {
                response,
                headers,
                trailers,
            };

            let args = parse_plugin_arguments(plugin_manager, arg_str, response, headers, trailers);

            match plugin.execute(&args, &context) {
                Ok(PluginResult::Value(v)) => return v,
                _ => return Value::Null,
            }
        }
    }
    resolve_path(expr, response)
}

fn parse_value(s: &str) -> Value {
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

fn parse_plugin_arguments(
    plugin_manager: &PluginManager,
    arg_str: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
) -> Vec<Value> {
    split_arguments(arg_str)
        .into_iter()
        .map(|token| parse_argument_value(plugin_manager, token, response, headers, trailers))
        .collect()
}

fn split_arguments(input: &str) -> Vec<&str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in trimmed.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => {
                out.push(trimmed[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }

    out.push(trimmed[start..].trim());
    out
}

fn parse_argument_value(
    plugin_manager: &PluginManager,
    token: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
) -> Value {
    let t = token.trim();
    if t.is_empty() {
        return Value::Null;
    }

    if t.starts_with('@') && t.contains('(') && t.ends_with(')') {
        return evaluate_expression(plugin_manager, t, response, headers, trailers);
    }

    if t == "." || t.starts_with('.') {
        return resolve_path(t, response);
    }

    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || t == "true"
        || t == "false"
        || t == "null"
        || t.parse::<f64>().is_ok()
    {
        return parse_value(t);
    }

    Value::String(t.to_string())
}

fn compare(
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

fn resolve_path(path: &str, root: &Value) -> Value {
    if path == "." {
        return root.clone();
    }

    let mut current = root;
    let clean_path = path.strip_prefix('.').unwrap_or(path);

    let mut parts = Vec::new();
    let mut start = 0;
    let chars: Vec<char> = clean_path.chars().collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_plugin_manager() -> PluginManager {
        PluginManager::new()
    }

    #[test]
    fn test_evaluate_assertion_equality() {
        let pm = create_plugin_manager();
        let response = json!({"status": "success"});
        let result =
            evaluate_assertion(&pm, ".status == \"success\"", &response, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_inequality() {
        let pm = create_plugin_manager();
        let response = json!({"status": "success"});
        let result =
            evaluate_assertion(&pm, ".status == \"error\"", &response, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_evaluate_assertion_contains() {
        let pm = create_plugin_manager();
        let response = json!({"name": "test"});
        let result =
            evaluate_assertion(&pm, ".name contains \"te\"", &response, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_plugin() {
        let pm = create_plugin_manager();
        let response = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        let result = evaluate_assertion(&pm, "@uuid(.id)", &response, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_has_header_unquoted_argument() {
        let pm = create_plugin_manager();
        let response = json!({});
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let result = evaluate_assertion(
            &pm,
            "@has_header(content-type) == true",
            &response,
            Some(&headers),
            None,
        )
        .unwrap();

        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_trailer_value_plugin() {
        let pm = create_plugin_manager();
        let response = json!({});
        let mut trailers = HashMap::new();
        trailers.insert("grpc-status".to_string(), "0".to_string());

        let result = evaluate_assertion(
            &pm,
            "@trailer(\"grpc-status\") == \"0\"",
            &response,
            None,
            Some(&trailers),
        )
        .unwrap();

        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_resolve_path_simple() {
        let response = json!({"key": "value"});
        let result = resolve_path(".key", &response);
        assert_eq!(result, json!("value"));
    }

    #[test]
    fn test_resolve_path_nested() {
        let response = json!({"outer": {"inner": "value"}});
        let result = resolve_path(".outer.inner", &response);
        assert_eq!(result, json!("value"));
    }

    #[test]
    fn test_parse_value_string() {
        assert_eq!(parse_value("\"hello\""), json!("hello"));
    }

    #[test]
    fn test_parse_value_number() {
        assert_eq!(parse_value("123"), json!(123));
    }

    #[test]
    fn test_parse_value_bool() {
        assert_eq!(parse_value("true"), json!(true));
        assert_eq!(parse_value("false"), json!(false));
    }
}
