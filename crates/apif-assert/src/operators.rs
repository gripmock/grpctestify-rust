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

type ValueResult = std::result::Result<Value, String>;

pub fn regex_with_flags(pattern: &str, flags: &str) -> String {
    let supported: String = flags
        .chars()
        .filter(|c| matches!(c, 'i' | 'm' | 's' | 'x' | 'u' | 'U'))
        .collect();
    if supported.is_empty() {
        pattern.to_string()
    } else {
        format!("(?{}){}", supported, pattern)
    }
}

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<String, std::result::Result<Rc<Regex>, String>>> =
        RefCell::new(HashMap::new());
}

pub fn cached_regex(pattern: &str) -> std::result::Result<Rc<Regex>, String> {
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

pub(crate) struct EvalCtx<'a> {
    pub response: &'a Value,
    pub headers: Option<&'a HashMap<String, String>>,
    pub trailers: Option<&'a HashMap<String, String>>,
    pub timing: Option<&'a AssertionTiming>,
    pub variables: &'a HashMap<String, Value>,
    pub protocol: Option<&'a str>,
}

impl<'a> EvalCtx<'a> {
    pub fn new(response: &'a Value, variables: &'a HashMap<String, Value>) -> Self {
        Self {
            response,
            headers: None,
            trailers: None,
            timing: None,
            variables,
            protocol: None,
        }
    }
    pub fn with_headers(mut self, headers: Option<&'a HashMap<String, String>>) -> Self {
        self.headers = headers;
        self
    }
    pub fn with_trailers(mut self, trailers: Option<&'a HashMap<String, String>>) -> Self {
        self.trailers = trailers;
        self
    }
    pub fn with_timing(mut self, timing: Option<&'a AssertionTiming>) -> Self {
        self.timing = timing;
        self
    }
    pub fn with_protocol(mut self, protocol: Option<&'a str>) -> Self {
        self.protocol = protocol;
        self
    }
}

pub(crate) fn evaluate_assertion(
    registry: &dyn PluginRegistry,
    assertion: &str,
    ctx: &EvalCtx,
) -> Result<Option<AssertionResult>> {
    let trimmed = assertion.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let ast = parse_assertion(trimmed);
    match &ast {
        AssertionExpr::Raw(_) => Ok(None),
        _ => evaluate_ast(registry, &ast, ctx).map(Some),
    }
}

fn evaluate_ast(
    pm: &dyn PluginRegistry,
    expr: &AssertionExpr,
    ctx: &EvalCtx,
) -> Result<AssertionResult> {
    match expr {
        AssertionExpr::Not(inner) => {
            let r = evaluate_ast(pm, inner, ctx)?;
            Ok(negate(r))
        }
        AssertionExpr::NotNot(inner) => evaluate_ast(pm, inner, ctx),
        AssertionExpr::And { left, right } => {
            let lr = evaluate_ast(pm, left, ctx)?;
            if !is_pass(&lr) {
                return Ok(AssertionResult::fail(format!(
                    "Left of 'and' failed: {}",
                    fmt_result_short(&lr)
                )));
            }
            let rr = evaluate_ast(pm, right, ctx)?;
            if !is_pass(&rr) {
                return Ok(AssertionResult::fail(format!(
                    "Right of 'and' failed: {}",
                    fmt_result_short(&rr)
                )));
            }
            Ok(AssertionResult::Pass)
        }
        AssertionExpr::Or { left, right } => {
            let lr = evaluate_ast(pm, left, ctx)?;
            if is_pass(&lr) {
                return Ok(AssertionResult::Pass);
            }
            let rr = evaluate_ast(pm, right, ctx)?;
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
            let lr = evaluate_ast(pm, left, ctx)?;
            let rr = evaluate_ast(pm, right, ctx)?;
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
            let lhs = match eval_value(pm, left, ctx) {
                Ok(v) => v,
                Err(e) => return Ok(AssertionResult::Error(e)),
            };
            let rhs = match eval_value(pm, right, ctx) {
                Ok(v) => v,
                Err(e) => return Ok(AssertionResult::Error(e)),
            };
            compare(lhs, op, rhs, left, right)
        }
        AssertionExpr::Paren(inner) => evaluate_ast(pm, inner, ctx),
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            let cond = evaluate_ast(pm, condition, ctx)?;
            if is_pass(&cond) {
                evaluate_ast(pm, then_branch, ctx)
            } else {
                evaluate_ast(pm, else_branch, ctx)
            }
        }
        AssertionExpr::Atom(_) => {
            if let AssertionExpr::Atom(Expr::PluginCall { name, args }) = expr {
                eval_plugin_as_assertion(pm, name, args, ctx)
            } else {
                let val = match eval_value(pm, expr, ctx) {
                    Ok(v) => v,
                    Err(e) => return Ok(AssertionResult::Error(e)),
                };
                if is_truthy(&val) {
                    Ok(AssertionResult::Pass)
                } else {
                    Ok(AssertionResult::fail(format!(
                        "Expression evaluated to falsy: {}",
                        val
                    )))
                }
            }
        }
        AssertionExpr::Raw(_) => Ok(AssertionResult::Error("Unparsed expression".into())),
    }
}

fn validate_type_cast(val: &Value, type_name: &str) -> Value {
    match type_name {
        "bool" => bool_or_null(val),
        "uint" => uint_or_null(val),
        "number" => number_or_null(val),
        "string" | "uuid" | "email" | "url" | "ip" => string_or_null(val),
        "time" | "timestamp" | "duration" => {
            if val.is_string() || val.is_number() {
                val.clone()
            } else {
                Value::Null
            }
        }
        "json" => {
            if val.is_object() || val.is_array() {
                val.clone()
            } else {
                Value::Null
            }
        }
        "yaml" => string_or_null(val),
        _ => val.clone(),
    }
}

fn bool_or_null(val: &Value) -> Value {
    if val.is_boolean() {
        val.clone()
    } else {
        Value::Null
    }
}

fn string_or_null(val: &Value) -> Value {
    if val.is_string() {
        val.clone()
    } else {
        Value::Null
    }
}

fn number_or_null(val: &Value) -> Value {
    if val.is_number() {
        return val.clone();
    }
    let Value::String(s) = val else {
        return Value::Null;
    };
    if let Ok(i) = s.parse::<i64>() {
        return Value::Number(i.into());
    }
    if let Ok(u) = s.parse::<u64>() {
        return Value::Number(u.into());
    }
    s.parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn uint_or_null(val: &Value) -> Value {
    if val.as_u64().is_some() {
        return val.clone();
    }
    let Value::String(s) = val else {
        return Value::Null;
    };
    s.parse::<u64>()
        .map(|u| Value::Number(u.into()))
        .unwrap_or(Value::Null)
}

fn eval_plugin_as_assertion(
    pm: &dyn PluginRegistry,
    name: &str,
    args: &[AssertionExpr],
    ctx: &EvalCtx,
) -> Result<AssertionResult> {
    let func_name = format!("@{}", name);
    let resolved_name = normalize_plugin_name(&func_name);
    if let Some(plugin) = pm.get_plugin(resolved_name) {
        let plugin_ctx = PluginContext::new(ctx.response)
            .with_headers(ctx.headers)
            .with_trailers(ctx.trailers)
            .with_timing(ctx.timing)
            .with_protocol(ctx.protocol);
        let arg_values: Vec<Value> = match args
            .iter()
            .map(|a| eval_value(pm, a, ctx))
            .collect::<std::result::Result<_, _>>()
        {
            Ok(values) => values,
            Err(e) => return Ok(AssertionResult::Error(e)),
        };
        match plugin.execute(&arg_values, &plugin_ctx) {
            Ok(PluginResult::Assertion(res)) => Ok(res),
            Ok(PluginResult::Value(val)) => {
                if is_truthy(&val) {
                    Ok(AssertionResult::Pass)
                } else {
                    Ok(AssertionResult::fail(format!(
                        "Plugin {} returned falsy value: {}",
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

fn eval_value(pm: &dyn PluginRegistry, expr: &AssertionExpr, ctx: &EvalCtx) -> ValueResult {
    match expr {
        AssertionExpr::Atom(atom) => eval_atom(pm, atom, ctx),
        AssertionExpr::Paren(inner) => eval_value(pm, inner, ctx),
        AssertionExpr::Not(inner) => {
            let v = eval_value(pm, inner, ctx)?;
            Ok(Value::Bool(!is_truthy(&v)))
        }
        AssertionExpr::NotNot(inner) => eval_value(pm, inner, ctx),
        AssertionExpr::And { left, right } => {
            let lv = eval_value(pm, left, ctx)?;
            if !is_truthy(&lv) {
                return Ok(Value::Bool(false));
            }
            let rv = eval_value(pm, right, ctx)?;
            Ok(Value::Bool(is_truthy(&rv)))
        }
        AssertionExpr::Or { left, right } => {
            let lv = eval_value(pm, left, ctx)?;
            if is_truthy(&lv) {
                return Ok(Value::Bool(true));
            }
            let rv = eval_value(pm, right, ctx)?;
            Ok(Value::Bool(is_truthy(&rv)))
        }
        AssertionExpr::Xor { left, right } => {
            let lv = eval_value(pm, left, ctx)?;
            let rv = eval_value(pm, right, ctx)?;
            Ok(Value::Bool(is_truthy(&lv) != is_truthy(&rv)))
        }
        AssertionExpr::Binary { op, left, right } => {
            let lhs = eval_value(pm, left, ctx)?;
            let rhs = eval_value(pm, right, ctx)?;
            Ok(eval_binary_value(lhs, op, rhs))
        }
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => {
            let cv = eval_value(pm, condition, ctx)?;
            if is_truthy(&cv) {
                eval_value(pm, then_branch, ctx)
            } else {
                eval_value(pm, else_branch, ctx)
            }
        }
        AssertionExpr::Raw(s) => Ok(resolve_path(s, ctx.response)),
    }
}

fn eval_atom(pm: &dyn PluginRegistry, atom: &Expr, ctx: &EvalCtx) -> ValueResult {
    match atom {
        Expr::JqPath(p) => Ok(resolve_path(p, ctx.response)),
        Expr::PluginCall { name, args } => {
            let func_name = format!("@{}", name);
            let resolved_name = normalize_plugin_name(&func_name);
            if let Some(plugin) = pm.get_plugin(resolved_name) {
                let plugin_ctx = PluginContext::new(ctx.response)
                    .with_headers(ctx.headers)
                    .with_trailers(ctx.trailers)
                    .with_timing(ctx.timing)
                    .with_protocol(ctx.protocol);
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| eval_value(pm, a, ctx))
                    .collect::<std::result::Result<_, _>>()?;
                match plugin.execute(&arg_values, &plugin_ctx) {
                    Ok(PluginResult::Value(v)) => Ok(v),
                    Ok(PluginResult::Assertion(AssertionResult::Pass)) => Ok(Value::Bool(true)),
                    Ok(PluginResult::Assertion(AssertionResult::Fail { .. })) => {
                        Ok(Value::Bool(false))
                    }
                    Ok(PluginResult::Assertion(AssertionResult::Error(e))) => {
                        Err(format!("Plugin {} error: {}", resolved_name, e))
                    }
                    Err(e) => Err(format!("Plugin {} error: {}", resolved_name, e)),
                }
            } else {
                Ok(Value::Null)
            }
        }
        Expr::Literal(lit) => Ok(match lit {
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
        }),
        Expr::Variable(name) => match ctx.variables.get(name.as_str()) {
            Some(v) => Ok(v.clone()),
            None => Err(format!("Undefined variable: ${}", name)),
        },
        Expr::RegExp { pattern, flags } => Ok(Value::String(regex_with_flags(pattern, flags))),
        Expr::Json(s) | Expr::Yaml(s) => Ok(serde_json::from_str(s).unwrap_or(Value::Null)),
        Expr::As(inner, type_name) => {
            let val = eval_atom(pm, inner, ctx)?;
            Ok(validate_type_cast(&val, type_name))
        }
    }
}

fn values_numerically_equal(lhs: &Value, rhs: &Value) -> bool {
    if let (Value::Number(l), Value::Number(r)) = (lhs, rhs) {
        if let (Some(li), Some(ri)) = (l.as_i64(), r.as_i64()) {
            return li == ri;
        }
        if let (Some(lu), Some(ru)) = (l.as_u64(), r.as_u64()) {
            return lu == ru;
        }
        if let (Some(lf), Some(rf)) = (l.as_f64(), r.as_f64()) {
            return lf == rf;
        }
        return l == r;
    }
    lhs == rhs
}

fn eval_binary_value(lhs: Value, op: &BinaryOp, rhs: Value) -> Value {
    let pass = match op {
        BinaryOp::Eq => values_numerically_equal(&lhs, &rhs),
        BinaryOp::Ne => !values_numerically_equal(&lhs, &rhs),
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

fn is_path(written: &str) -> bool {
    written.starts_with('.')
        && written
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '[' | ']' | '"'))
}

fn number_as_string_hint(
    lhs: &Value,
    rhs: &Value,
    left_expr: &AssertionExpr,
    right_expr: &AssertionExpr,
) -> Option<String> {
    let numeric = |v: &Value| matches!(v, Value::Number(_));
    let numeric_text = |v: &Value| match v {
        Value::String(s) => s.trim().parse::<f64>().is_ok() && !s.trim().is_empty(),
        _ => false,
    };
    let path = if numeric_text(lhs) && numeric(rhs) {
        left_expr.to_string()
    } else if numeric(lhs) && numeric_text(rhs) {
        right_expr.to_string()
    } else {
        return None;
    };
    is_path(&path).then(|| {
        format!(
            "the answer holds it as a string; compare with `{}:number` (protobuf sends 64-bit integers that way)",
            path
        )
    })
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
        let hint = number_as_string_hint(&lhs, &rhs, left_expr, right_expr);
        Ok(AssertionResult::Fail {
            message: format!(
                "Assertion failed: {} {} {} (Values: {} vs {}){}",
                left_expr,
                op.as_str(),
                right_expr,
                lhs,
                rhs,
                hint.as_deref()
                    .map(|h| format!(" — {h}"))
                    .unwrap_or_default()
            ),
            expected: Some(format!("{} {}", op.as_str(), rhs)),
            actual: Some(lhs.to_string()),
            hint,
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
        let empty = HashMap::new();
        evaluate_assertion(pm, expr, &EvalCtx::new(response, &empty))
            .unwrap()
            .unwrap_or(AssertionResult::Error("AST returned None".into()))
    }

    fn eval_with_vars(
        pm: &dyn PluginRegistry,
        expr: &str,
        response: &Value,
        variables: &HashMap<String, Value>,
    ) -> AssertionResult {
        evaluate_assertion(pm, expr, &EvalCtx::new(response, variables))
            .unwrap()
            .unwrap_or(AssertionResult::Error("AST returned None".into()))
    }

    #[test]
    fn equality_pass() {
        let r = eval(
            &pm(),
            ".status == \"success\"",
            &json!({"status": "success"}),
        );
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn number_type_annotation_coerces_int64_string_field() {
        let r = eval(
            &pm(),
            ".big_id:number > 100",
            &json!({"big_id": "123456789012345"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "{r:?}");
    }

    #[test]
    fn equality_fail() {
        let r = eval(&pm(), ".status == \"error\"", &json!({"status": "success"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn contains() {
        let r = eval(&pm(), ".name contains \"te\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn xor_both_true() {
        let r = eval(&pm(), ".x == 1 xor .y == 2", &json!({"x": 1, "y": 2}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn xor_both_false() {
        let r = eval(&pm(), ".x == 9 xor .y == 9", &json!({"x": 1, "y": 2}));
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn numeric_greater() {
        let r = eval(&pm(), ".id > 100", &json!({"id": 123}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn numeric_less() {
        let r = eval(&pm(), ".id < 200", &json!({"id": 123}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn matches_regex() {
        let r = eval(&pm(), ".name matches \"^te.*t$\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Pass));
    }

    #[test]
    fn matches_regex_fail() {
        let r = eval(&pm(), ".name matches \"^xyz\"", &json!({"name": "test"}));
        assert!(matches!(r, AssertionResult::Fail { .. }));
    }

    #[test]
    fn jq_fallback_via_raw() {
        let p = pm();
        let response = json!({"tags": [1, 2, 3]});
        let empty = HashMap::new();
        let r = evaluate_assertion(&p, ".tags | length", &EvalCtx::new(&response, &empty)).unwrap();
        assert!(
            r.is_none(),
            "JQ pipe should return None to trigger JQ fallback"
        );
    }

    #[test]
    fn resolve_path_simple() {
        let r = resolve_path(".key", &json!({"key": "value"}));
        assert_eq!(r, json!("value"));
    }

    #[test]
    fn resolve_path_nested() {
        let r = resolve_path(".outer.inner", &json!({"outer": {"inner": "value"}}));
        assert_eq!(r, json!("value"));
    }

    #[test]
    fn resolve_path_array_index() {
        let r = resolve_path(".items[0]", &json!({"items": ["first", "second"]}));
        assert_eq!(r, json!("first"));
    }

    #[test]
    fn resolve_path_missing_key() {
        let r = resolve_path(".missing", &json!({"a": 1}));
        assert!(r.is_null());
    }

    #[test]
    fn compare_numeric_greater() {
        assert_eq!(compare_numeric(&json!(5), &json!(3), ">"), Some(true));
    }

    #[test]
    fn compare_numeric_less() {
        assert_eq!(compare_numeric(&json!(3), &json!(5), "<"), Some(true));
    }

    #[test]
    fn compare_numeric_equality() {
        assert_eq!(compare_numeric(&json!(5), &json!(5), ">="), Some(true));
        assert_eq!(compare_numeric(&json!(5), &json!(5), "<="), Some(true));
    }

    #[test]
    fn compare_numeric_mixed_types() {
        assert_eq!(compare_numeric(&json!(5), &json!("5"), ">"), None);
    }

    #[test]
    fn cached_regex_valid() {
        assert!(cached_regex(r"\d+").is_ok());
    }

    #[test]
    fn cached_regex_invalid() {
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
    fn validate_type_cast_coerces_numeric_strings() {
        assert_eq!(
            validate_type_cast(&json!("123456789012345"), "number"),
            json!(123456789012345i64)
        );
        assert_eq!(validate_type_cast(&json!("42"), "uint"), json!(42u64));
        assert_eq!(validate_type_cast(&json!("2.5"), "number"), json!(2.5));
        assert_eq!(validate_type_cast(&json!("-5"), "number"), json!(-5));
        assert_eq!(validate_type_cast(&json!("-5"), "uint"), Value::Null);
        assert_eq!(validate_type_cast(&json!("hello"), "number"), Value::Null);
        assert_eq!(validate_type_cast(&json!("hello"), "uint"), Value::Null);
    }

    #[test]
    fn normalize_plugin_name_assert() {
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
    fn eval_atom_literal() {
        let pm = crate::registry::NoopPluginRegistry;
        let response = json!({});
        let empty = HashMap::new();
        use apif_ast::assertion_ast::{Expr, Literal};
        let result = eval_atom(
            &pm,
            &Expr::Literal(Literal::Number("42".into())),
            &EvalCtx::new(&response, &empty),
        )
        .unwrap();
        assert_eq!(result, json!(42));
    }

    #[test]
    fn test_regex_with_flags() {
        assert_eq!(regex_with_flags("^te.*t$", ""), "^te.*t$");
        assert_eq!(regex_with_flags("^te.*t$", "i"), "(?i)^te.*t$");
        assert_eq!(regex_with_flags("^te.*t$", "im"), "(?im)^te.*t$");
        assert_eq!(regex_with_flags("^te.*t$", "gi"), "(?i)^te.*t$");
        assert_eq!(regex_with_flags("^te.*t$", "g"), "^te.*t$");
    }

    #[test]
    fn matches_regex_honors_case_insensitive_flag() {
        use apif_ast::assertion_ast::{BinaryOp, Expr};
        let expr = AssertionExpr::Binary {
            op: BinaryOp::Matches,
            left: Box::new(AssertionExpr::Atom(Expr::JqPath(".name".into()))),
            right: Box::new(AssertionExpr::Atom(Expr::RegExp {
                pattern: "^TE.*T$".into(),
                flags: "i".into(),
            })),
        };
        let response = json!({"name": "test"});
        let empty = HashMap::new();
        let r = evaluate_ast(&pm(), &expr, &EvalCtx::new(&response, &empty)).unwrap();
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);

        let expr = AssertionExpr::Binary {
            op: BinaryOp::Matches,
            left: Box::new(AssertionExpr::Atom(Expr::JqPath(".name".into()))),
            right: Box::new(AssertionExpr::Atom(Expr::RegExp {
                pattern: "^TE.*T$".into(),
                flags: String::new(),
            })),
        };
        let r = evaluate_ast(&pm(), &expr, &EvalCtx::new(&response, &empty)).unwrap();
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    struct ErrorPlugin;

    impl crate::registry::PluginApi for ErrorPlugin {
        fn execute(
            &self,
            _args: &[Value],
            _context: &PluginContext,
        ) -> anyhow::Result<crate::registry::PluginResult> {
            Ok(crate::registry::PluginResult::Assertion(
                AssertionResult::Error("boom".into()),
            ))
        }
    }

    struct ErrorPluginRegistry;

    impl PluginRegistry for ErrorPluginRegistry {
        fn get_plugin(&self, name: &str) -> Option<std::sync::Arc<dyn crate::registry::PluginApi>> {
            (name == "err").then(|| {
                std::sync::Arc::new(ErrorPlugin) as std::sync::Arc<dyn crate::registry::PluginApi>
            })
        }
    }

    #[test]
    fn plugin_error_in_value_position_propagates() {
        let r = eval(
            &ErrorPluginRegistry,
            "@err(.x) == \"error: boom\"",
            &json!({"x": 1}),
        );
        assert!(matches!(r, AssertionResult::Error(_)), "got: {:?}", r);

        let r = eval(&ErrorPluginRegistry, "@err(.x) != 1", &json!({"x": 1}));
        assert!(matches!(r, AssertionResult::Error(_)), "got: {:?}", r);
    }

    #[test]
    fn eval_binary_value_num() {
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

    #[test]
    fn eq_is_exact_on_large_integers() {
        let r = eval(
            &pm(),
            ".id == 9223372036854775807",
            &json!({"id": 9223372036854775806i64}),
        );
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);

        let r = eval(
            &pm(),
            ".id == 9223372036854775807",
            &json!({"id": 9223372036854775807i64}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);

        let r = eval(
            &pm(),
            ".id != 9223372036854775807",
            &json!({"id": 9223372036854775806i64}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn extract_variable_resolves_in_assertion() {
        let mut vars = HashMap::new();
        vars.insert("price".to_string(), json!(42));
        let r = eval_with_vars(&pm(), "$price >= 0", &json!({}), &vars);
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);

        vars.insert("price".to_string(), json!(-5));
        let r = eval_with_vars(&pm(), "$price >= 0", &json!({}), &vars);
        assert!(matches!(r, AssertionResult::Fail { .. }), "got: {:?}", r);
    }

    #[test]
    fn extract_variable_string_contains() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), json!("hello world"));
        let r = eval_with_vars(&pm(), "$name contains \"hello\"", &json!({}), &vars);
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }

    #[test]
    fn unbound_variable_errors() {
        let vars = HashMap::new();
        let r = eval_with_vars(&pm(), "$missing >= 0", &json!({}), &vars);
        match r {
            AssertionResult::Error(msg) => assert!(msg.contains("missing"), "msg: {}", msg),
            other => panic!("expected Error for unbound variable, got: {:?}", other),
        }
    }

    #[test]
    fn a_number_that_came_back_as_a_string_names_the_cast() {
        let r = eval(&pm(), ".expires_in == 3600", &json!({"expires_in": "3600"}));
        match r {
            AssertionResult::Fail { message, .. } => {
                assert!(
                    message.contains("`.expires_in:number`"),
                    "message: {message}"
                );
                assert!(
                    message.contains("holds it as a string"),
                    "message: {message}"
                );
            }
            other => panic!("expected Fail, got: {other:?}"),
        }
    }

    #[test]
    fn the_cast_it_names_is_the_one_that_passes() {
        let r = eval(
            &pm(),
            ".expires_in:number == 3600",
            &json!({"expires_in": "3600"}),
        );
        assert!(matches!(r, AssertionResult::Pass), "got: {r:?}");
    }

    #[test]
    fn an_ordinary_mismatch_says_nothing_about_casts() {
        let r = eval(
            &pm(),
            ".status == \"NOT_SERVING\"",
            &json!({"status": "SERVING"}),
        );
        match r {
            AssertionResult::Fail { message, .. } => {
                assert!(!message.contains("number"), "message: {message}");
            }
            other => panic!("expected Fail, got: {other:?}"),
        }
        let r = eval(&pm(), ".name == 3", &json!({"name": "ada"}));
        match r {
            AssertionResult::Fail { message, .. } => {
                assert!(!message.contains(":number"), "message: {message}");
            }
            other => panic!("expected Fail, got: {other:?}"),
        }
    }

    #[test]
    fn a_computed_side_is_offered_no_cast() {
        let r = eval(&pm(), "@len(.items) == 3", &json!({"items": "3"}));
        match r {
            AssertionResult::Fail { message, .. } => {
                assert!(!message.contains(":number"), "message: {message}");
            }
            AssertionResult::Error(_) => {}
            other => panic!("expected Fail or Error, got: {other:?}"),
        }
    }

    #[test]
    fn eq_int_vs_float_still_equal_by_value() {
        let r = eval(&pm(), ".x == 3.0", &json!({"x": 3}));
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
        let r = eval(&pm(), ".x == 3", &json!({"x": 3.0}));
        assert!(matches!(r, AssertionResult::Pass), "got: {:?}", r);
    }
}
