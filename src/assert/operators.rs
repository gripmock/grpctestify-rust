// AST-based assertion engine
// All evaluation goes through the AssertionExpr AST — no string-based parsing.

use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assert::engine::AssertionResult;
use crate::parser::assertion_ast::{AssertionExpr, BinaryOp, Expr, Literal, parse_assertion};
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

/// Evaluate an assertion expression.
/// Returns `Ok(Some(result))` when the AST engine handled the expression,
/// `Ok(None)` when the expression should fall through to the JQ evaluator.
pub fn evaluate_assertion(
    plugin_manager: &PluginManager,
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
        _ => evaluate_ast(plugin_manager, &ast, response, headers, trailers, timing).map(Some),
    }
}

fn evaluate_ast(
    pm: &PluginManager,
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
fn eval_plugin_as_assertion(
    pm: &PluginManager,
    name: &str,
    args: &[AssertionExpr],
    response: &Value,
    headers: Option<&HashMap<String, String>>,
    trailers: Option<&HashMap<String, String>>,
    timing: Option<&AssertionTiming>,
) -> Result<AssertionResult> {
    let func_name = format!("@{}", name);
    let resolved_name = normalize_plugin_name(&func_name);
    if let Some(plugin) = pm.get(resolved_name) {
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
    pm: &PluginManager,
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
    pm: &PluginManager,
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
            if let Some(plugin) = pm.get(resolved_name) {
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
        Expr::Variable(name) => Value::String(format!("{{{{{}}}}}", name)),
        Expr::RegExp { pattern, flags: _ } => Value::String(format!("/{}/", pattern)),
        Expr::Json(s) | Expr::Yaml(s) => serde_json::from_str(s).unwrap_or(Value::Null),
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
            (Value::String(l), Value::String(r)) => {
                cached_regex(r).map(|re| re.is_match(l)).unwrap_or(false)
            }
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

    fn pm() -> PluginManager {
        PluginManager::new()
    }

    fn eval(pm: &PluginManager, expr: &str, response: &Value) -> AssertionResult {
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
    fn test_plugin_uuid_pass() {
        let r = eval(
            &pm(),
            "@uuid(.id)",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_plugin_uuid_fail() {
        let r = eval(&pm(), "@uuid(.id)", &json!({"id": "not-a-uuid"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_plugin_email_pass() {
        let r = eval(
            &pm(),
            "@email(.email)",
            &json!({"email": "test@example.com"}),
        );
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn test_plugin_email_fail() {
        let r = eval(&pm(), "@email(.email)", &json!({"email": "not-an-email"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn test_negation_plugin_non_empty() {
        let r = eval(&pm(), "!@empty(.id)", &json!({"id": "user_1001"}));
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_negation_plugin_empty() {
        let r = eval(&pm(), "!@empty(.id)", &json!({"id": ""}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_negation_not_keyword() {
        let r = eval(&pm(), "not @empty(.id)", &json!({"id": "user_1001"}));
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_double_negation_bang() {
        let r = eval(
            &pm(),
            "!!@uuid(.id)",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_double_negation_not_not() {
        let r = eval(
            &pm(),
            "not not @uuid(.id)",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_pipe_not_non_empty() {
        let r = eval(&pm(), "@empty(.id) | not", &json!({"id": "user_1001"}));
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_pipe_not_empty() {
        let r = eval(&pm(), "@empty(.id) | not", &json!({"id": ""}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_pipe_not_not() {
        let r = eval(&pm(), "@empty(.id) | not not", &json!({"id": "user_1001"}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_or_first_true() {
        let r = eval(
            &pm(),
            "@uuid(.id) or @email(.id)",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_or_second_true() {
        let r = eval(
            &pm(),
            "@uuid(.id) or @email(.id)",
            &json!({"id": "test@example.com"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_or_both_false() {
        let r = eval(
            &pm(),
            "@uuid(.id) or @email(.id)",
            &json!({"id": "not-valid"}),
        );
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_xor_one_true() {
        let r = eval(
            &pm(),
            "@uuid(.id) xor @email(.id)",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
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
    fn test_and_both_true() {
        let r = eval(
            &pm(),
            "@uuid(.id) and .id == \"550e8400-e29b-41d4-a716-446655440000\"",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_and_left_false() {
        let r = eval(
            &pm(),
            "@uuid(.id) and .id == \"wrong\"",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_paren_or_in_and() {
        let r = eval(
            &pm(),
            "(@uuid(.a) or @email(.b)) and .c == 1",
            &json!({"a": "550e8400-e29b-41d4-a716-446655440000", "b": "x", "c": 1}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_negated_paren_or() {
        let r = eval(
            &pm(),
            "!(@uuid(.id) or @email(.id))",
            &json!({"id": "550e8400-e29b-41d4-a716-446655440000"}),
        );
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn test_negated_paren_or_both_false() {
        let r = eval(
            &pm(),
            "!(@uuid(.id) or @email(.id))",
            &json!({"id": "garbage"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_has_header_with_headers() {
        let p = pm();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let r = evaluate_assertion(
            &p,
            "@has_header(\"content-type\") == true",
            &json!({}),
            Some(&headers),
            None,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn test_trailer_value_plugin() {
        let p = pm();
        let mut trailers = HashMap::new();
        trailers.insert("grpc-status".to_string(), "0".to_string());
        let r = evaluate_assertion(
            &p,
            "@trailer(\"grpc-status\") == \"0\"",
            &json!({}),
            None,
            Some(&trailers),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(r, AssertionResult::Pass));
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
}
