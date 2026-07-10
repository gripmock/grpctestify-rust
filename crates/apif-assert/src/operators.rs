// AST-based assertion engine
// All evaluation goes through the AssertionExpr AST — no string-based parsing.

use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::engine::AssertionResult;
use crate::registry::{AssertionTiming, PluginContext, PluginRegistry, PluginResult};
use apif_ast::assertion_ast::{AssertionExpr, BinaryOp, Expr, Literal, parse_assertion};
fn normalize_plugin_name(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.strip_prefix('@').unwrap_or(trimmed)
}

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

/// Evaluate an assertion expression.
/// Returns `Ok(Some(result))` when the AST engine handled the expression,
/// `Ok(None)` when the expression should fall through to the JQ evaluator.
pub fn evaluate_assertion(
    registry: &dyn PluginRegistry,
    assertion: &str,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Result<Option<AssertionResult>> {
    let trimmed = assertion.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let ast = parse_assertion(trimmed);
    match &ast {
        AssertionExpr::Raw(_) => Ok(None),
        _ => evaluate_ast(registry, &ast, response, headers, trailers, timing).map(Some),
    }
}

fn evaluate_ast(
    pm: &dyn PluginRegistry,
    expr: &AssertionExpr,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Result<AssertionResult> {
    match expr {
        AssertionExpr::Not(inner) => {
            let r = evaluate_ast(pm, inner, response, headers, trailers, timing)?;
            Ok(negate(r))
        }
        AssertionExpr::NotNot(inner) => {
            evaluate_ast(pm, inner, response, headers, trailers, timing)
        }
        AssertionExpr::And { left, right } => {
            let lr = evaluate_ast(pm, left, response, headers, trailers, timing)?;
            if !is_pass(&lr) {
                return Ok(AssertionResult::fail(format!(
                    "Left of 'and' failed: {}",
                    fmt_result_short(&lr)
                )));
            }
            let rr = evaluate_ast(pm, right, response, headers, trailers, timing)?;
            if !is_pass(&rr) {
                return Ok(AssertionResult::fail(format!(
                    "Right of 'and' failed: {}",
                    fmt_result_short(&rr)
                )));
            }
            Ok(AssertionResult::Pass)
        }
        AssertionExpr::Or { left, right } => {
            let lr = evaluate_ast(pm, left, response, headers, trailers, timing)?;
            if is_pass(&lr) {
                return Ok(AssertionResult::Pass);
            }
            let rr = evaluate_ast(pm, right, response, headers, trailers, timing)?;
            if is_pass(&rr) {
                return Ok(AssertionResult::Pass);
            }
            Ok(AssertionResult::fail(format!(
                "Both sides of 'or' failed: left={}, right={}",
                fmt_result_short(&lr),
                fmt_result_short(&rr)
            )))
        }
        AssertionExpr::Xor { left, right } => {
            let lr = evaluate_ast(pm, left, response, headers, trailers, timing)?;
            let rr = evaluate_ast(pm, right, response, headers, trailers, timing)?;
            let lp = is_pass(&lr);
            let rp = is_pass(&rr);
            if lp != rp {
                Ok(AssertionResult::Pass)
            } else {
                Ok(AssertionResult::fail(format!(
                    "Xor expects exactly one true, got left={} right={}",
                    lp, rp
                )))
            }
        }
        AssertionExpr::Binary { op, left, right } => {
            let lhs = eval_value(pm, left, response, headers, trailers, timing);
            let rhs = eval_value(pm, right, response, headers, trailers, timing);
            compare(lhs, op, rhs, left, right)
        }
        AssertionExpr::Paren(inner) => evaluate_ast(pm, inner, response, headers, trailers, timing),
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = evaluate_ast(pm, condition, response, headers, trailers, timing)?;
            if is_pass(&cond) {
                evaluate_ast(pm, then_branch, response, headers, trailers, timing)
            } else {
                evaluate_ast(pm, else_branch, response, headers, trailers, timing)
            }
        }
        AssertionExpr::Atom(_) => {
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = expr {
                eval_plugin_as_assertion(pm, name, args, response, headers, trailers, timing)
            } else {
                let val = eval_value(pm, expr, response, headers, trailers, timing);
                if is_truthy(&val) {
                    Ok(AssertionResult::Pass)
                } else {
                    Ok(AssertionResult::fail(format!(
                        "Expression evaluated to falsy: {:?}",
                        val
                    )))
                }
            }
        }
        AssertionExpr::Raw(_) => Ok(AssertionResult::Error("Unparsed expression".into())),
    }
}

/// Evaluate an AST node as a JSON value (for use inside Binary, plugin args, etc.)
/// Validate a value against a type annotation.
/// Returns `Value::Null` if the value doesn't match the expected type,
/// otherwise returns the value unchanged.
fn validate_type_cast(val: &Value, type_name: &str) -> Value {
    let valid = match type_name {
        "bool" => val.is_boolean(),
        "uint" => val.as_u64().is_some(),
        "number" => val.is_number(),
        "string" | "uuid" | "email" | "url" | "ip" => val.is_string(),
        "time" | "timestamp" | "duration" => val.is_string() || val.is_number(),
        "json" => val.is_object() || val.is_array(),
        "yaml" => val.is_string(),
        _ => true,
    };
    if valid { val.clone() } else { Value::Null }
}

fn eval_plugin_as_assertion(
    pm: &dyn PluginRegistry,
    name: &str,
    args: &[AssertionExpr],
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Result<AssertionResult> {
    let func_name = format!("@{}", name);
    let resolved_name = normalize_plugin_name(&func_name);
    if let Some(plugin) = pm.get_plugin(resolved_name) {
        let ctx = PluginContext::new(response)
            .with_headers(headers)
            .with_trailers(trailers)
            .with_timing(timing);
        let arg_values: Vec<Value> = args
            .iter()
            .map(|a| eval_value(pm, a, response, headers, trailers, timing))
            .collect();
        match plugin.execute(&arg_values, &ctx) {
            Ok(PluginResult::Assertion(res)) => Ok(res),
            Ok(PluginResult::Value(val)) => {
                if is_truthy(&val) {
                    Ok(AssertionResult::Pass)
                } else {
                    Ok(AssertionResult::fail(format!(
                        "Plugin {} returned falsy value: {:?}",
                        resolved_name, val
                    )))
                }
            }
            Err(e) => Ok(AssertionResult::Error(format!("Plugin error: {}", e))),
        }
    } else {
        Ok(AssertionResult::Error(format!("Unknown plugin: {}", name)))
    }
}

fn eval_value(
    pm: &dyn PluginRegistry,
    expr: &AssertionExpr,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Value {
    match expr {
        AssertionExpr::Atom(atom) => eval_atom(pm, atom, response, headers, trailers, timing),
        AssertionExpr::Paren(inner) => eval_value(pm, inner, response, headers, trailers, timing),
        AssertionExpr::Not(inner) => {
            let v = eval_value(pm, inner, response, headers, trailers, timing);
            Value::Bool(!is_truthy(&v))
        }
        AssertionExpr::NotNot(inner) => eval_value(pm, inner, response, headers, trailers, timing),
        AssertionExpr::And { left, right } => {
            let lv = eval_value(pm, left, response, headers, trailers, timing);
            if !is_truthy(&lv) {
                return Value::Bool(false);
            }
            let rv = eval_value(pm, right, response, headers, trailers, timing);
            Value::Bool(is_truthy(&rv))
        }
        AssertionExpr::Or { left, right } => {
            let lv = eval_value(pm, left, response, headers, trailers, timing);
            if is_truthy(&lv) {
                return Value::Bool(true);
            }
            let rv = eval_value(pm, right, response, headers, trailers, timing);
            Value::Bool(is_truthy(&rv))
        }
        AssertionExpr::Xor { left, right } => {
            let lv = eval_value(pm, left, response, headers, trailers, timing);
            let rv = eval_value(pm, right, response, headers, trailers, timing);
            Value::Bool(is_truthy(&lv) != is_truthy(&rv))
        }
        AssertionExpr::Binary { op, left, right } => {
            let lhs = eval_value(pm, left, response, headers, trailers, timing);
            let rhs = eval_value(pm, right, response, headers, trailers, timing);
            eval_binary_value(lhs, op, rhs)
        }
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            let cv = eval_value(pm, condition, response, headers, trailers, timing);
            if is_truthy(&cv) {
                eval_value(pm, then_branch, response, headers, trailers, timing)
            } else {
                eval_value(pm, else_branch, response, headers, trailers, timing)
            }
        }
        AssertionExpr::Raw(s) => resolve_path(s, response),
    }
}

fn eval_atom(
    pm: &dyn PluginRegistry,
    atom: &Expr,
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Value {
    match atom {
        Expr::JqPath(p) => resolve_path(p, response),
        Expr::PluginCall { name, args } => {
            let func_name = format!("@{}", name);
            let resolved_name = normalize_plugin_name(&func_name);
            if let Some(plugin) = pm.get_plugin(resolved_name) {
                let ctx = PluginContext::new(response)
                    .with_headers(headers)
                    .with_trailers(trailers)
                    .with_timing(timing);
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| eval_value(pm, a, response, headers, trailers, timing))
                    .collect();
                match plugin.execute(&arg_values, &ctx) {
                    Ok(PluginResult::Value(v)) => v,
                    Ok(PluginResult::Assertion(AssertionResult::Pass)) => Value::Bool(true),
                    Ok(PluginResult::Assertion(AssertionResult::Fail { .. })) => Value::Bool(false),
                    Ok(PluginResult::Assertion(AssertionResult::Error(e))) => {
                        Value::String(format!("error: {}", e))
                    }
                    Err(_) => Value::Null,
                }
            } else {
                Value::Null
            }
        }
        Expr::Literal(lit) => match lit {
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Number(n) => n
                .parse::<i64>()
                .map(|i| Value::Number(serde_json::Number::from(i)))
                .unwrap_or_else(|_| {
                    n.parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map(Value::Number)
                        .unwrap_or(Value::Null)
                }),
            Literal::Str(s) => Value::String(s.clone()),
            Literal::Null => Value::Null,
        },
        Expr::Variable(name) => Value::String(format!("${}", name)),
        Expr::RegExp { pattern, flags: _ } => Value::String(pattern.clone()),
        Expr::Json(s) | Expr::Yaml(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        Expr::As(inner, type_name) => {
            let val = eval_atom(pm, inner, response, headers, trailers, timing);
            validate_type_cast(&val, type_name)
        }
    }
}

fn eval_binary_value(lhs: Value, op: &BinaryOp, rhs: Value) -> Value {
    let pass = match op {
        BinaryOp::Eq => lhs == rhs,
        BinaryOp::Ne => lhs != rhs,
        BinaryOp::Gt => compare_numeric(&lhs, &rhs, ">").unwrap_or(false),
        BinaryOp::Lt => compare_numeric(&lhs, &rhs, "<").unwrap_or(false),
        BinaryOp::Ge => compare_numeric(&lhs, &rhs, ">=").unwrap_or(false),
        BinaryOp::Le => compare_numeric(&lhs, &rhs, "<=").unwrap_or(false),
        BinaryOp::Contains => match (&lhs, &rhs) {
            (Value::String(l), Value::String(r)) => l.contains(r),
            (Value::Array(l), r) => l.contains(r),
            (Value::Object(l), Value::String(r)) => l.contains_key(r),
            _ => false,
        },
        BinaryOp::StartsWith => match (&lhs, &rhs) {
            (Value::String(l), Value::String(r)) => l.starts_with(r),
            _ => false,
        },
        BinaryOp::EndsWith => match (&lhs, &rhs) {
            (Value::String(l), Value::String(r)) => l.ends_with(r),
            _ => false,
        },
        BinaryOp::Matches => match (&lhs, &rhs) {
            (Value::String(l), Value::String(r)) => cached_regex(r).is_ok_and(|re| re.is_match(l)),
            _ => false,
        },
    };
    Value::Bool(pass)
}

fn compare(
    lhs: Value,
    op: &BinaryOp,
    rhs: Value,
    left_expr: &AssertionExpr,
    right_expr: &AssertionExpr,
) -> Result<AssertionResult> {
    if let BinaryOp::Matches = op
        && let (Value::String(_l), Value::String(r)) = (&lhs, &rhs)
        && cached_regex(r).is_err()
    {
        return Ok(AssertionResult::Error(format!("Invalid regex: {}", r)));
    }
    let pass = eval_binary_value(lhs.clone(), op, rhs.clone());
    if pass == Value::Bool(true) {
        Ok(AssertionResult::Pass)
    } else {
        Ok(AssertionResult::Fail {
            message: format!(
                "Assertion failed: {} {} {} (Values: {:?} vs {:?})",
                left_expr,
                op.as_str(),
                right_expr,
                lhs,
                rhs
            ),
            expected: Some(format!("{} {:?}", op.as_str(), rhs)),
            actual: Some(format!("{:?}", lhs)),
        })
    }
}

fn compare_numeric(lhs: &Value, rhs: &Value, op: &str) -> Option<bool> {
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

fn resolve_path(path: &str, root: &Value) -> Value {
    if path == "." {
        return root.clone();
    }
    if path.is_empty() {
        return Value::Null;
    }
    if !path.starts_with('.') && !path.starts_with('$') {
        return Value::String(path.to_string());
    }
    eval_jaq_one(path, root).unwrap_or(Value::Null)
}

fn eval_jaq_one(expr: &str, input: &Value) -> anyhow::Result<Value> {
    super::engine::AssertionEngine::eval_jaq_one(expr, input)
}

fn is_truthy(val: &Value) -> bool {
    !val.is_null() && val != &Value::Bool(false)
}

fn is_pass(r: &AssertionResult) -> bool {
    matches!(r, AssertionResult::Pass)
}

fn negate(r: AssertionResult) -> AssertionResult {
    r.negate()
}

fn fmt_result_short(r: &AssertionResult) -> String {
    match r {
        AssertionResult::Pass => "pass".into(),
        AssertionResult::Fail { message, .. } => message.clone(),
        AssertionResult::Error(e) => format!("error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pm() -> crate::registry::NoopPluginRegistry {
        crate::registry::NoopPluginRegistry
    }

    fn eval(pm: &dyn PluginRegistry, expr: &str, response: &Value) -> AssertionResult {
        evaluate_assertion(pm, expr, response, None, None, None)
            .unwrap()
            .unwrap_or(AssertionResult::Error("AST returned None".into()))
    }

    #[test]
    fn test_equality_pass() {
        let r = eval(
            &pm(),
            ".status == \"success\"",
            &json!({"status": "success"}),
        );
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_equality_fail() {
        let r = eval(&pm(), ".status == \"error\"", &json!({"status": "success"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_contains() {
        let r = eval(&pm(), ".name contains \"te\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_xor_both_true() {
        let r = eval(&pm(), ".x == 1 xor .y == 2", &json!({"x": 1, "y": 2}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_xor_both_false() {
        let r = eval(&pm(), ".x == 9 xor .y == 9", &json!({"x": 1, "y": 2}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_numeric_greater() {
        let r = eval(&pm(), ".id > 100", &json!({"id": 123}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_numeric_less() {
        let r = eval(&pm(), ".id < 200", &json!({"id": 123}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_matches_regex() {
        let r = eval(&pm(), ".name matches \"^te.*t$\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_matches_regex_fail() {
        let r = eval(&pm(), ".name matches \"^xyz\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_jq_fallback_via_raw() {
        let p = pm();
        let r = evaluate_assertion(
            &p,
            ".tags | length",
            &json!({"tags": [1, 2, 3]}),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            r.is_none(),
            "JQ pipe should return None to trigger JQ fallback"
        );
    }

    #[test]
    fn test_resolve_path_simple() {
        let r = resolve_path(".key", &json!({"key": "value"}));
        assert_eq!(r, json!("value"));
    }

    #[test]
    fn test_resolve_path_nested() {
        let r = resolve_path(".outer.inner", &json!({"outer": {"inner": "value"}}));
        assert_eq!(r, json!("value"));
    }

    #[test]
    fn test_resolve_path_array_index() {
        let r = resolve_path(".items[0]", &json!({"items": ["first", "second"]}));
        assert_eq!(r, json!("first"));
    }

    #[test]
    fn test_resolve_path_missing_key() {
        let r = resolve_path(".missing", &json!({"a": 1}));
        assert!(r.is_null());
    }

    #[test]
    fn test_compare_numeric_greater() {
        assert_eq!(compare_numeric(&json!(5), &json!(3), ">"), Some(true));
    }

    #[test]
    fn test_compare_numeric_less() {
        assert_eq!(compare_numeric(&json!(3), &json!(5), "<"), Some(true));
    }

    #[test]
    fn test_compare_numeric_equality() {
        assert_eq!(compare_numeric(&json!(5), &json!(5), ">="), Some(true));
        assert_eq!(compare_numeric(&json!(5), &json!(5), "<="), Some(true));
    }

    #[test]
    fn test_compare_numeric_mixed_types() {
        assert_eq!(compare_numeric(&json!(5), &json!("5"), ">"), None);
    }

    #[test]
    fn test_cached_regex_valid() {
        assert!(cached_regex(r"\d+").is_ok());
    }

    #[test]
    fn test_cached_regex_invalid() {
        assert!(cached_regex(r"[").is_err());
    }

    #[test]
    fn test_validate_type_cast() {
        use serde_json::json;
        assert_eq!(validate_type_cast(&json!(42), "number"), json!(42));
        assert_eq!(validate_type_cast(&json!("hello"), "number"), Value::Null);
        assert_eq!(
            validate_type_cast(&json!("hello"), "string"),
            json!("hello")
        );
        assert_eq!(validate_type_cast(&json!(42), "string"), Value::Null);
        assert_eq!(validate_type_cast(&json!(true), "bool"), json!(true));
        assert_eq!(validate_type_cast(&json!("hello"), "bool"), Value::Null);
        assert_eq!(validate_type_cast(&json!(42u64), "uint"), json!(42u64));
        assert_eq!(validate_type_cast(&json!(-1), "uint"), Value::Null);
        assert_eq!(
            validate_type_cast(&json!("uuid-str"), "uuid"),
            json!("uuid-str")
        );
        assert_eq!(
            validate_type_cast(&json!("email@x.com"), "email"),
            json!("email@x.com")
        );
        assert_eq!(validate_type_cast(&json!("url"), "url"), json!("url"));
        assert_eq!(
            validate_type_cast(&json!("1.2.3.4"), "ip"),
            json!("1.2.3.4")
        );
        assert_eq!(
            validate_type_cast(&json!("2024-01-01"), "time"),
            json!("2024-01-01")
        );
        assert_eq!(validate_type_cast(&json!(12345), "timestamp"), json!(12345));
        assert_eq!(
            validate_type_cast(&json!("100ms"), "duration"),
            json!("100ms")
        );
        assert_eq!(
            validate_type_cast(&json!({"k": "v"}), "json"),
            json!({"k": "v"})
        );
        assert_eq!(validate_type_cast(&json!([1, 2]), "json"), json!([1, 2]));
        assert_eq!(validate_type_cast(&json!("hello"), "json"), Value::Null);
        assert_eq!(
            validate_type_cast(&json!("yaml:val"), "yaml"),
            json!("yaml:val")
        );
        assert_eq!(
            validate_type_cast(&json!("any_val"), "unknown_type"),
            json!("any_val")
        );
    }

    #[test]
    fn test_normalize_plugin_name_assert() {
        assert_eq!(normalize_plugin_name("@uuid"), "uuid");
        assert_eq!(normalize_plugin_name("uuid"), "uuid");
        assert_eq!(normalize_plugin_name(" @uuid "), "uuid");
    }

    #[test]
    fn test_is_truthy() {
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&Value::Bool(false)));
        assert!(is_truthy(&Value::Bool(true)));
        assert!(is_truthy(&Value::Number(0.into())));
        assert!(is_truthy(&Value::String("".into())));
    }

    #[test]
    fn test_negate() {
        let pass = AssertionResult::Pass;
        assert!(matches!(negate(pass), AssertionResult::Fail { .. }));

        let fail = AssertionResult::fail("msg");
        assert!(matches!(negate(fail), AssertionResult::Pass));

        let err = AssertionResult::Error("err".into());
        assert!(matches!(negate(err), AssertionResult::Error(_)));
    }

    #[test]
    fn test_fmt_result_short() {
        assert_eq!(fmt_result_short(&AssertionResult::Pass), "pass");
        assert_eq!(fmt_result_short(&AssertionResult::fail("msg")), "msg");
        assert_eq!(
            fmt_result_short(&AssertionResult::Error("err".into())),
            "error: err"
        );
    }

    #[test]
    fn test_eval_atom_literal() {
        let pm = crate::registry::NoopPluginRegistry;
        let ctx = &json!({});
        use apif_ast::assertion_ast::{Expr, Literal};
        let result = eval_atom(
            &pm,
            &Expr::Literal(Literal::Number("42".into())),
            ctx,
            None,
            None,
            None,
        );
        assert_eq!(result, json!(42));
    }

    #[test]
    fn test_eval_binary_value_num() {
        use apif_ast::assertion_ast::BinaryOp;
        assert_eq!(
            eval_binary_value(json!(5), &BinaryOp::Gt, json!(3)),
            json!(true)
        );
        assert_eq!(
            eval_binary_value(json!(3), &BinaryOp::Gt, json!(5)),
            json!(false)
        );
    }
}
