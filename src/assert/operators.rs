// Operators assertion engine
// Supports @plugin functions and custom operators (==, !=, contains, etc.)

use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    AssertionTiming, PluginContext, PluginManager, PluginResult, normalize_plugin_name,
};

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<String, std::result::Result<Rc<Regex>, String>>> =
        RefCell::new(HashMap::new());
}

fn cached_regex(pattern: &str) -> std::result::Result<Rc<Regex>, String> {
    if let Some(cached) = REGEX_CACHE.with(|cache| cache.borrow().get(pattern).cloned()) {
        return cached;
    }

    let compiled = Regex::new(pattern)
        .map(Rc::new)
        .map_err(|err| err.to_string());

    REGEX_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(pattern.to_string(), compiled.clone());
    });

    compiled
}

/// Evaluate assertion expression (plugins and operators)
pub fn evaluate_assertion(
    plugin_manager: &PluginManager,
    assertion: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Result<AssertionResult> {
    let trimmed = assertion.trim();

    // Check for built-in boolean functions first (e.g. @uuid(.id))
    if trimmed.starts_with('@')
        && !trimmed.contains("==")
        && !trimmed.contains("!=")
        && !trimmed.contains('>')
        && !trimmed.contains('<')
    {
        return evaluate_boolean_function(
            plugin_manager,
            trimmed,
            response,
            headers,
            trailers,
            timing,
        );
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

            let lhs_val =
                evaluate_expression(plugin_manager, lhs_str, response, headers, trailers, timing);
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
    timing: Option<&AssertionTiming>,
) -> Result<AssertionResult> {
    if let (Some(start_paren), Some(end_paren)) = (expr.find('('), expr.rfind(')')) {
        let func_name = &expr[0..start_paren];
        let arg_str = &expr[start_paren + 1..end_paren];

        let resolved_name = normalize_plugin_name(func_name);

        if let Some(plugin) = plugin_manager.get(resolved_name) {
            let context = PluginContext::new(response)
                .with_headers(headers)
                .with_trailers(trailers)
                .with_timing(timing);

            let args = parse_plugin_arguments(
                plugin_manager,
                arg_str,
                response,
                headers,
                trailers,
                timing,
            );

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
    timing: Option<&AssertionTiming>,
) -> Value {
    if expr.starts_with('@')
        && let (Some(start_paren), Some(end_paren)) = (expr.find('('), expr.rfind(')'))
    {
        let func_name = &expr[0..start_paren];
        let arg_str = &expr[start_paren + 1..end_paren];

        let resolved_name = normalize_plugin_name(func_name);

        if let Some(plugin) = plugin_manager.get(resolved_name) {
            let context = PluginContext::new(response)
                .with_headers(headers)
                .with_trailers(trailers)
                .with_timing(timing);

            let args = parse_plugin_arguments(
                plugin_manager,
                arg_str,
                response,
                headers,
                trailers,
                timing,
            );

            match plugin.execute(&args, &context) {
                Ok(PluginResult::Value(v)) => return v,
                _ => return Value::Null,
            }
        }
    }
    resolve_path(expr, response)
}

fn parse_value(s: &str) -> Value {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = s.trim_matches('"');
        Value::String(inner.to_string())
    } else if s == "true" {
        Value::Bool(true)
    } else if s == "false" {
        Value::Bool(false)
    } else if s == "null" {
        Value::Null
    } else if let Ok(i) = s.parse::<i64>() {
        Value::Number(serde_json::Number::from(i))
    } else if let Ok(f) = s.parse::<f64>() {
        serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else {
        Value::String(s.to_string())
    }
}

fn compare_numeric_values(lhs: &Value, rhs: &Value, op: &str) -> Option<bool> {
    let lhs_num = lhs.as_number()?;
    let rhs_num = rhs.as_number()?;

    let lhs_i = lhs_num
        .as_i64()
        .map(i128::from)
        .or_else(|| lhs_num.as_u64().map(i128::from));
    let rhs_i = rhs_num
        .as_i64()
        .map(i128::from)
        .or_else(|| rhs_num.as_u64().map(i128::from));

    if let (Some(l), Some(r)) = (lhs_i, rhs_i) {
        return Some(match op {
            ">" => l > r,
            "<" => l < r,
            ">=" => l >= r,
            "<=" => l <= r,
            _ => return None,
        });
    }

    let (l, r) = (lhs_num.as_f64()?, rhs_num.as_f64()?);
    Some(match op {
        ">" => l > r,
        "<" => l < r,
        ">=" => l >= r,
        "<=" => l <= r,
        _ => return None,
    })
}

fn parse_plugin_arguments(
    plugin_manager: &PluginManager,
    arg_str: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Vec<Value> {
    split_arguments(arg_str)
        .into_iter()
        .map(|token| {
            parse_argument_value(plugin_manager, token, response, headers, trailers, timing)
        })
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
    timing: Option<&AssertionTiming>,
) -> Value {
    let t = token.trim();
    if t.is_empty() {
        return Value::Null;
    }

    if t.starts_with('@') && t.contains('(') && t.ends_with(')') {
        return evaluate_expression(plugin_manager, t, response, headers, trailers, timing);
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
        ">" => compare_numeric_values(&lhs, &rhs, op).unwrap_or(false),
        "<" => compare_numeric_values(&lhs, &rhs, op).unwrap_or(false),
        ">=" => compare_numeric_values(&lhs, &rhs, op).unwrap_or(false),
        "<=" => compare_numeric_values(&lhs, &rhs, op).unwrap_or(false),
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
                if let Ok(re) = cached_regex(r) {
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

/// Evaluate a JQ path expression using the jaq engine.
/// Delegates to jaq's native parser — handles all JQ syntax:
///   .foo["bar"].baz, .arr[0], .obj["key"]["nested"], etc.
fn resolve_path(path: &str, root: &Value) -> Value {
    if path == "." {
        return root.clone();
    }
    eval_jaq_one(path, root).unwrap_or(Value::Null)
}

/// Evaluate a JQ expression and return the first result as serde_json::Value.
fn eval_jaq_one(expr: &str, input: &Value) -> anyhow::Result<Value> {
    use jaq_core::defs as core_defs;
    use jaq_core::funs as core_funs;
    use jaq_core::{Compiler, Ctx, Vars, data, load, unwrap_valr};
    use jaq_json::Val as JaqVal;

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
        .map_err(|errs| anyhow::anyhow!("JQ parse error: {:?}", errs))?;

    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|errs| anyhow::anyhow!("JQ compile error: {:?}", errs))?;

    let jaq_input = to_jaq_val(input);
    let ctx = Ctx::<data::JustLut<JaqVal>>::new(&filter.lut, Vars::new([]));
    let mut out = filter.id.run((ctx, jaq_input)).map(unwrap_valr);

    if let Some(Ok(val)) = out.next() {
        Ok(from_jaq_val(&val))
    } else {
        Err(anyhow::anyhow!("JQ produced no output"))
    }
}

/// Convert serde_json::Value → jaq_json::Val
fn to_jaq_val(v: &Value) -> jaq_json::Val {
    use jaq_json::Num as JaqNum;
    match v {
        Value::Null => jaq_json::Val::Null,
        Value::Bool(b) => jaq_json::Val::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                #[allow(clippy::cast_possible_wrap)]
                let isize_val = i as isize;
                jaq_json::Val::Num(JaqNum::Int(isize_val))
            } else if let Some(f) = n.as_f64() {
                jaq_json::Val::Num(JaqNum::Float(f))
            } else {
                jaq_json::Val::Null
            }
        }
        Value::String(s) => jaq_json::Val::utf8_str(bytes::Bytes::from(s.clone())),
        Value::Array(arr) => {
            jaq_json::Val::Arr(std::rc::Rc::new(arr.iter().map(to_jaq_val).collect()))
        }
        Value::Object(map) => {
            let entries: Vec<(jaq_json::Val, jaq_json::Val)> = map
                .iter()
                .map(|(k, v)| {
                    (
                        jaq_json::Val::utf8_str(bytes::Bytes::from(k.clone())),
                        to_jaq_val(v),
                    )
                })
                .collect();
            jaq_json::Val::Obj(std::rc::Rc::new(jaq_json::Map::from_iter(entries)))
        }
    }
}

/// Convert jaq_json::Val → serde_json::Value
fn from_jaq_val(v: &jaq_json::Val) -> Value {
    match v {
        jaq_json::Val::Null => Value::Null,
        jaq_json::Val::Bool(b) => Value::Bool(*b),
        jaq_json::Val::Num(n) => match n {
            jaq_json::Num::Int(i) => Value::Number(serde_json::Number::from(*i as i64)),
            jaq_json::Num::BigInt(_) => Value::Null,
            jaq_json::Num::Float(f) => serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            jaq_json::Num::Dec(f) => f
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        },
        jaq_json::Val::BStr(b) | jaq_json::Val::TStr(b) => {
            String::from_utf8_lossy(b).into_owned().into()
        }
        jaq_json::Val::Arr(arr) => Value::Array(arr.iter().map(from_jaq_val).collect()),
        jaq_json::Val::Obj(map) => {
            let entries: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        jaq_json::Val::TStr(b) | jaq_json::Val::BStr(b) => {
                            String::from_utf8_lossy(b).into_owned()
                        }
                        _ => k.to_string(),
                    };
                    (key, from_jaq_val(v))
                })
                .collect();
            Value::Object(entries)
        }
    }
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
            evaluate_assertion(&pm, ".status == \"success\"", &response, None, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_inequality() {
        let pm = create_plugin_manager();
        let response = json!({"status": "success"});
        let result =
            evaluate_assertion(&pm, ".status == \"error\"", &response, None, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_evaluate_assertion_contains() {
        let pm = create_plugin_manager();
        let response = json!({"name": "test"});
        let result =
            evaluate_assertion(&pm, ".name contains \"te\"", &response, None, None, None).unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_evaluate_assertion_plugin() {
        let pm = create_plugin_manager();
        let response = json!({"id": "550e8400-e29b-41d4-a716-446655440000"});
        let result = evaluate_assertion(&pm, "@uuid(.id)", &response, None, None, None).unwrap();
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
            None,
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

    #[test]
    fn test_cached_regex_valid() {
        let result = cached_regex(r"\d+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_cached_regex_invalid() {
        let result = cached_regex(r"[");
        assert!(result.is_err());
    }

    #[test]
    fn test_compare_numeric_greater() {
        let lhs = json!(5);
        let rhs = json!(3);
        assert_eq!(compare_numeric_values(&lhs, &rhs, ">"), Some(true));
    }

    #[test]
    fn test_compare_numeric_less() {
        let lhs = json!(3);
        let rhs = json!(5);
        assert_eq!(compare_numeric_values(&lhs, &rhs, "<"), Some(true));
    }

    #[test]
    fn test_compare_numeric_equality() {
        let lhs = json!(5);
        let rhs = json!(5);
        assert_eq!(compare_numeric_values(&lhs, &rhs, ">="), Some(true));
        assert_eq!(compare_numeric_values(&lhs, &rhs, "<="), Some(true));
    }

    #[test]
    fn test_compare_numeric_mixed_types() {
        let lhs = json!(5);
        let rhs = json!("5");
        assert_eq!(compare_numeric_values(&lhs, &rhs, ">"), None);
    }

    #[test]
    fn test_resolve_path_array_index() {
        let root = json!({"items": ["first", "second"]});
        let result = resolve_path(".items[0]", &root);
        assert_eq!(result, json!("first"));
    }

    #[test]
    fn test_resolve_path_missing_key() {
        let root = json!({"a": 1});
        let result = resolve_path(".missing", &root);
        assert!(result.is_null());
    }

    #[test]
    fn test_split_arguments_simple() {
        let args = split_arguments("arg1, arg2, arg3");
        assert_eq!(args.len(), 3);
    }

    #[test]
    fn test_split_arguments_empty() {
        let args = split_arguments("");
        assert!(args.is_empty());
    }

    #[test]
    fn test_split_arguments_with_parens() {
        let args = split_arguments("@len(.x), @empty(.y)");
        assert_eq!(args.len(), 2);
    }
}
