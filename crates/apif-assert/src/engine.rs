use anyhow::Result;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{LazyLock, Mutex};

use crate::registry::AssertionTiming;

use jaq_core::{
    Bind, Compiler, Ctx, Cv, Error as JaqError, Vars, data, load, native::bome, unwrap_valr,
};
use jaq_json::{Map as JaqMap, Num as JaqNum, Rc as JaqRc, Val as JaqVal};

use super::operators;

fn load_error_message(errors: &[(load::File<&str, ()>, load::Error<&str>)]) -> String {
    let Some((_, first)) = errors.first() else {
        return "jq could not read this filter".to_string();
    };
    match first {
        load::Error::Io(problems) => match problems.first() {
            Some((what, why)) => format!("jq could not read {what}: {why}"),
            None => "jq could not read this filter".to_string(),
        },
        load::Error::Lex(problems) => match problems.first() {
            Some((expected, rest)) => expecting(expected.as_str(), rest),
            None => "jq could not read this filter".to_string(),
        },
        load::Error::Parse(problems) => match problems.first() {
            Some((expected, rest)) => expecting(expected.as_str(), rest),
            None => "jq could not read this filter".to_string(),
        },
    }
}

type CompileMiss<'a> = (
    load::File<&'a str, ()>,
    Vec<jaq_core::compile::Error<&'a str>>,
);

fn compile_error_message(errors: &[CompileMiss<'_>]) -> String {
    match errors.first().and_then(|(_, undefined)| undefined.first()) {
        Some((name, what)) => format!("jq has no {} named `{name}`", what.as_str()),
        None => "jq could not compile this filter".to_string(),
    }
}

fn expecting(expected: &str, rest: &str) -> String {
    let rest = rest.trim();
    if rest.is_empty() {
        format!("jq expected {expected} and the filter ended")
    } else {
        let shown: String = rest.chars().take(24).collect();
        let shown = if rest.chars().count() > 24 {
            format!("{shown}…")
        } else {
            shown
        };
        format!("jq expected {expected} at `{shown}`")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalLayer {
    Ast,
    Jq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionResult {
    Pass,
    Fail {
        message: String,
        expected: Option<String>,
        actual: Option<String>,
        hint: Option<String>,
    },
    Error(String),
}

impl AssertionResult {
    pub fn fail(message: impl Into<String>) -> Self {
        Self::Fail {
            message: message.into(),
            expected: None,
            actual: None,
            hint: None,
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
            hint: None,
        }
    }

    pub fn negate(self) -> Self {
        match self {
            Self::Pass => Self::fail("Negated assertion passed (expected false)"),
            Self::Fail { .. } => Self::Pass,
            Self::Error(e) => Self::Error(e),
        }
    }
}

pub struct AssertionEngine {
    plugin_registry: Arc<dyn crate::registry::PluginRegistry>,
}

type JaqFilter = jaq_core::Filter<data::JustLut<JaqVal>>;

static JAQ_FILTER_CACHE: LazyLock<Mutex<HashMap<String, Arc<JaqFilter>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub const DENIED_JQ_FUNS: &[&str] = &["env"];

const JAQ_CONTEXT_ONLY_PLUGINS: &[&str] = &[
    "header",
    "has_header",
    "trailer",
    "has_trailer",
    "status",
    "elapsed_ms",
    "total_elapsed_ms",
    "env",
    "scope.message_count",
    "scope.index",
    "scope_message_count",
    "scope_index",
];

thread_local! {
    static JAQ_PLUGIN_REGISTRY: RefCell<Option<Arc<dyn crate::registry::PluginRegistry>>> =
        const { RefCell::new(None) };
}

struct PluginRegistryGuard(Option<Arc<dyn crate::registry::PluginRegistry>>);

impl PluginRegistryGuard {
    fn set(registry: Arc<dyn crate::registry::PluginRegistry>) -> Self {
        let prev = JAQ_PLUGIN_REGISTRY.with(|cell| cell.borrow_mut().replace(registry));
        Self(prev)
    }
}

impl Drop for PluginRegistryGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        JAQ_PLUGIN_REGISTRY.with(|cell| *cell.borrow_mut() = prev);
    }
}

fn dispatch_jaq_plugin(name: &str, args: &[Value]) -> std::result::Result<JaqVal, String> {
    let registry = JAQ_PLUGIN_REGISTRY.with(|cell| cell.borrow().clone());
    let registry =
        registry.ok_or_else(|| format!("plugin '@{}' is not available in this context", name))?;
    let plugin = registry
        .get_plugin(name)
        .ok_or_else(|| format!("unknown plugin '@{}' in jq expression", name))?;

    let null = Value::Null;
    let ctx = crate::registry::PluginContext::new(&null);
    match plugin
        .execute(args, &ctx)
        .map_err(|e| format!("plugin '@{}' error: {}", name, e))?
    {
        crate::registry::PluginResult::Value(v) => Ok(json_to_jaq(&v)),
        crate::registry::PluginResult::Assertion(AssertionResult::Pass) => Ok(JaqVal::Bool(true)),
        crate::registry::PluginResult::Assertion(AssertionResult::Fail { .. }) => {
            Ok(JaqVal::Bool(false))
        }
        crate::registry::PluginResult::Assertion(AssertionResult::Error(e)) => {
            Err(format!("plugin '@{}' error: {}", name, e))
        }
    }
}

fn jaq_plugin_fun<D>() -> jaq_core::native::Fun<D>
where
    D: for<'a> jaq_core::DataT<V<'a> = JaqVal>,
{
    jaq_core::native::run((
        "__plugin",
        Box::new([Bind::Fun(()), Bind::Fun(())]),
        |mut cv: Cv<D>| {
            let input = cv.1.clone();
            let (args_id, args_ctx) = cv.0.pop_fun();
            let (name_id, name_ctx) = cv.0.pop_fun();

            let name = match name_id
                .run((name_ctx, input.clone()))
                .map(unwrap_valr)
                .next()
            {
                Some(Ok(v)) => v,
                Some(Err(e)) => return bome(Err(e)),
                None => return bome(Err(JaqError::str("plugin call produced no name"))),
            };
            let name = match jaq_to_json(&name) {
                Value::String(s) => s,
                other => {
                    return bome(Err(JaqError::str(format!(
                        "plugin name must be a string, got {}",
                        other
                    ))));
                }
            };

            let args_val = match args_id.run((args_ctx, input)).map(unwrap_valr).next() {
                Some(Ok(v)) => v,
                Some(Err(e)) => return bome(Err(e)),
                None => {
                    return bome(Err(JaqError::str(format!(
                        "plugin '@{}' produced no arguments",
                        name
                    ))));
                }
            };
            let args_json = match jaq_to_json(&args_val) {
                Value::Array(items) => items,
                other => vec![other],
            };

            match dispatch_jaq_plugin(&name, &args_json) {
                Ok(v) => bome(Ok(v)),
                Err(e) => bome(Err(JaqError::str(e))),
            }
        },
    ))
}

fn rewrite_plugin_calls(expr: &str) -> Result<String> {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' | b'\'' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    let end = bytes[i] == b;
                    i += 1;
                    if end {
                        break;
                    }
                }
                out.push_str(&expr[start..i.min(bytes.len())]);
            }
            b'@' => {
                let name_start = i + 1;
                let mut j = name_start;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'.')
                {
                    j += 1;
                }
                if j > name_start && j < bytes.len() && bytes[j] == b'(' {
                    let name = &expr[name_start..j];
                    if JAQ_CONTEXT_ONLY_PLUGINS.contains(&name) {
                        return Err(anyhow::anyhow!(
                            "@{} is not available in jq expressions: it needs response \
                             header/trailer/timing/env context; use it as a standalone assertion",
                            name
                        ));
                    }
                    let close = find_matching_paren(bytes, j).ok_or_else(|| {
                        anyhow::anyhow!("unbalanced parentheses in plugin call @{}", name)
                    })?;
                    let inner = rewrite_plugin_calls(&expr[j + 1..close])?;
                    out.push_str("__plugin(\"");
                    out.push_str(name);
                    out.push_str("\"; [");
                    out.push_str(&inner);
                    out.push_str("])");
                    i = close + 1;
                } else {
                    out.push('@');
                    i += 1;
                }
            }
            _ => {
                let len = utf8_char_len(b);
                out.push_str(&expr[i..(i + len).min(bytes.len())]);
                i += len;
            }
        }
    }
    Ok(out)
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

fn find_matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match in_string {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_string = None;
                }
            }
            None => match b {
                b'"' | b'\'' => in_string = Some(b),
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

impl AssertionEngine {
    pub fn new() -> Self {
        Self {
            plugin_registry: Arc::new(crate::registry::NoopPluginRegistry),
        }
    }

    pub fn with_registry(registry: Arc<dyn crate::registry::PluginRegistry>) -> Self {
        Self {
            plugin_registry: registry,
        }
    }

    pub fn evaluate(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Result<AssertionResult> {
        self.evaluate_with_timing(
            assertion,
            response,
            headers,
            trailers,
            None,
            &HashMap::new(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_with_timing(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
        protocol: Option<&str>,
    ) -> Result<AssertionResult> {
        self.evaluate_with_timing_layered(
            assertion, response, headers, trailers, timing, variables, protocol,
        )
        .map(|(result, _)| result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_with_timing_layered(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
        protocol: Option<&str>,
    ) -> Result<(AssertionResult, EvalLayer)> {
        let trimmed = assertion.trim();

        let ctx = operators::EvalCtx::new(response, variables)
            .with_headers(headers)
            .with_trailers(trailers)
            .with_timing(timing)
            .with_protocol(protocol);

        match operators::evaluate_assertion(&*self.plugin_registry, trimmed, &ctx) {
            Ok(Some(result)) => Ok((result, EvalLayer::Ast)),
            Ok(None) => {
                if let Some(pos) = find_lone_equals(trimmed) {
                    return Ok((
                        AssertionResult::fail(format!(
                            "Assertion uses `=` at position {} — did you mean `==`? \
                         (`=` is not a comparison operator): {}",
                            pos, trimmed
                        )),
                        EvalLayer::Ast,
                    ));
                }
                if let Some(op) = apif_ast::assertion_ast::dangling_operator(trimmed) {
                    return Ok((
                        AssertionResult::fail(if op == "|" {
                            format!("Assertion ends on `|` with nothing after it: {trimmed}")
                        } else {
                            format!(
                                "Assertion ends on `{op}` with nothing to compare against: {trimmed}"
                            )
                        }),
                        EvalLayer::Ast,
                    ));
                }
                Ok((self.evaluate_jaq(trimmed, response)?, EvalLayer::Jq))
            }
            Err(e) => Err(e),
        }
    }

    pub fn query(&self, expr: &str, input: &Value) -> Result<Vec<Value>> {
        let values = self.run_jaq(expr, input)?;
        Ok(values.iter().map(jaq_to_json).collect())
    }

    pub fn query_bounded(
        &self,
        expr: &str,
        input: &Value,
        max_outputs: usize,
        timeout: std::time::Duration,
    ) -> Result<Vec<Value>> {
        let deadline = std::time::Instant::now() + timeout;
        let values = self.run_jaq_bounded(expr, input, Some(max_outputs), Some(deadline))?;
        Ok(values.iter().map(jaq_to_json).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_bounded(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
        protocol: Option<&str>,
        max_outputs: usize,
        timeout: std::time::Duration,
    ) -> Result<(AssertionResult, EvalLayer)> {
        let trimmed = assertion.trim();
        let ctx = operators::EvalCtx::new(response, variables)
            .with_headers(headers)
            .with_trailers(trailers)
            .with_timing(timing)
            .with_protocol(protocol);

        match operators::evaluate_assertion(&*self.plugin_registry, trimmed, &ctx) {
            Ok(Some(result)) => Ok((result, EvalLayer::Ast)),
            Ok(None) => {
                if let Some(pos) = find_lone_equals(trimmed) {
                    return Ok((
                        AssertionResult::fail(format!(
                            "Assertion uses `=` at position {} — did you mean `==`?",
                            pos
                        )),
                        EvalLayer::Ast,
                    ));
                }
                let deadline = std::time::Instant::now() + timeout;
                let out = match self.run_jaq_bounded(
                    trimmed,
                    response,
                    Some(max_outputs),
                    Some(deadline),
                ) {
                    Ok(out) => out,
                    Err(e) => return Ok((AssertionResult::Error(e.to_string()), EvalLayer::Jq)),
                };
                Ok((jaq_outputs_to_result(trimmed, &out), EvalLayer::Jq))
            }
            Err(e) => Err(e),
        }
    }

    fn evaluate_jaq(&self, expr: &str, response: &Value) -> Result<AssertionResult> {
        let out = match self.run_jaq(expr, response) {
            Ok(out) => out,
            Err(e) => return Ok(AssertionResult::Error(format!("JQ Parse Error: {}", e))),
        };
        Ok(jaq_outputs_to_result(expr, &out))
    }

    fn run_jaq(&self, expr: &str, input: &Value) -> Result<Vec<JaqVal>> {
        self.run_jaq_bounded(expr, input, None, None)
    }

    fn run_jaq_bounded(
        &self,
        expr: &str,
        input: &Value,
        max_outputs: Option<usize>,
        deadline: Option<std::time::Instant>,
    ) -> Result<Vec<JaqVal>> {
        let rewritten = rewrite_plugin_calls(expr)?;
        let filter = Self::get_or_compile_jaq_filter(&rewritten)?;

        let input = json_to_jaq(input);

        let _registry_guard = PluginRegistryGuard::set(self.plugin_registry.clone());

        let ctx = Ctx::<data::JustLut<JaqVal>>::new(&filter.lut, Vars::new([]));
        let out = filter.id.run((ctx, input)).map(unwrap_valr);

        let mut values = Vec::new();
        for item in out {
            match item {
                Ok(v) => values.push(v),
                Err(e) => return Err(anyhow::anyhow!("JQ Runtime Error: {}", e)),
            }
            if let Some(limit) = max_outputs
                && values.len() > limit
            {
                return Err(anyhow::anyhow!(
                    "expression produced more than {} outputs — jq generators like \
                     `repeat`/`until` never end",
                    limit
                ));
            }
            if let Some(deadline) = deadline
                && std::time::Instant::now() >= deadline
            {
                return Err(anyhow::anyhow!(
                    "expression did not finish in time — jq generators like `repeat`/`until` \
                     never end"
                ));
            }
        }

        Ok(values)
    }

    fn get_or_compile_jaq_filter(expr: &str) -> Result<Arc<JaqFilter>> {
        use jaq_core::defs as core_defs;
        use jaq_core::funs as core_funs;

        if let Some(cached) = JAQ_FILTER_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(expr)
            .cloned()
        {
            return Ok(cached);
        }

        let cleaned = strip_numeric_underscores(expr);

        let arena = load::Arena::default();
        let defs = core_defs().chain(jaq_std::defs()).chain(jaq_json::defs());
        let funs = core_funs()
            .chain(jaq_std::funs().filter(|f| !DENIED_JQ_FUNS.contains(&f.0)))
            .chain(jaq_json::funs())
            .chain(std::iter::once(jaq_plugin_fun()));
        let loader = load::Loader::new(defs);
        let program = load::File {
            code: cleaned.as_str(),
            path: (),
        };

        let modules = loader
            .load(&arena, program)
            .map_err(|errs| anyhow::anyhow!("{}", load_error_message(&errs)))?;

        let filter = Compiler::default()
            .with_funs(funs)
            .compile(modules)
            .map_err(|errs| anyhow::anyhow!("{}", compile_error_message(&errs)))?;

        let filter = Arc::new(filter);
        JAQ_FILTER_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(expr.to_string(), Arc::clone(&filter));

        Ok(filter)
    }

    pub(super) fn eval_jaq_one(expr: &str, input: &Value) -> anyhow::Result<Value> {
        let filter = Self::get_or_compile_jaq_filter(expr)?;
        let jaq_input = json_to_jaq(input);
        let ctx = Ctx::<data::JustLut<JaqVal>>::new(&filter.lut, Vars::new([]));
        let mut out = filter.id.run((ctx, jaq_input)).map(unwrap_valr);
        if let Some(Ok(val)) = out.next() {
            Ok(jaq_to_json(&val))
        } else {
            Err(anyhow::anyhow!("JQ produced no output for: {}", expr))
        }
    }

    #[must_use]
    pub fn has_failures(&self, results: &[AssertionResult]) -> bool {
        results
            .iter()
            .any(|r| matches!(r, AssertionResult::Fail { .. } | AssertionResult::Error(_)))
    }

    pub fn get_failures<'a>(&self, results: &'a [AssertionResult]) -> Vec<&'a AssertionResult> {
        results
            .iter()
            .filter(|r| matches!(r, AssertionResult::Fail { .. } | AssertionResult::Error(_)))
            .collect()
    }

    pub fn evaluate_all(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> Vec<AssertionResult> {
        self.evaluate_all_with_timing(
            assertions,
            response,
            headers,
            trailers,
            None,
            &HashMap::new(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_all_with_timing(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
        protocol: Option<&str>,
    ) -> Vec<AssertionResult> {
        self.evaluate_all_with_records(
            assertions, response, headers, trailers, timing, variables, protocol,
        )
        .into_iter()
        .map(|(result, _elapsed_ms)| result)
        .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_all_with_records(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
        protocol: Option<&str>,
    ) -> Vec<(AssertionResult, u64)> {
        assertions
            .iter()
            .map(|assertion| {
                let start = std::time::Instant::now();
                let result = self
                    .evaluate_with_timing(
                        assertion, response, headers, trailers, timing, variables, protocol,
                    )
                    .unwrap_or_else(|e| AssertionResult::Error(format!("Internal error: {}", e)));
                tracing::trace!("assertion: {assertion} -> {result:?}");
                (result, start.elapsed().as_millis() as u64)
            })
            .collect()
    }
}

fn strip_numeric_underscores(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut chars = expr.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '"' {
            out.push(c);
            while let Some(next) = chars.next() {
                out.push(next);
                if next == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                } else if next == '"' {
                    break;
                }
            }
        } else {
            let is_digit_separator = c == '_'
                && out.chars().next_back().is_some_and(|p| p.is_ascii_digit())
                && chars.peek().is_some_and(|n| n.is_ascii_digit());
            if !is_digit_separator {
                out.push(c);
            }
        }
    }

    out
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
                if let Some(i) = n.as_isize() {
                    Value::Number(serde_json::Number::from(i))
                } else {
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
            JaqNum::Dec(s) => s
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        },
        JaqVal::TStr(s) | JaqVal::BStr(s) => match std::str::from_utf8(s.as_ref()) {
            Ok(v) => Value::String(v.to_string()),
            Err(_) => Value::Null,
        },
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

fn find_lone_equals(expr: &str) -> Option<usize> {
    let bytes = expr.as_bytes();
    let mut in_string: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match in_string {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_string = None;
                }
            }
            None => match b {
                b'"' | b'\'' => in_string = Some(b),
                b'=' => {
                    let prev = if i > 0 { bytes[i - 1] } else { 0 };
                    let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
                    let is_double = next == b'=' || prev == b'=';
                    let is_compound = matches!(prev, b'!' | b'<' | b'>');
                    if !is_double && !is_compound {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn jaq_outputs_to_result(expr: &str, out: &[JaqVal]) -> AssertionResult {
    for val in out {
        if matches!(val, JaqVal::Bool(false) | JaqVal::Null) {
            let rendered = serde_json::to_string(&jaq_to_json(val))
                .unwrap_or_else(|_| "<unprintable>".to_string());
            return AssertionResult::fail(format!(
                "JQ assertion evaluated to falsy value {}: {}",
                rendered, expr
            ));
        }
    }
    if out.is_empty() {
        AssertionResult::fail(format!(
            "JQ assertion produced no output (falsey): {}",
            expr
        ))
    } else {
        AssertionResult::Pass
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
    fn a_filter_that_does_not_parse_says_what_is_missing() {
        let said = match AssertionEngine::get_or_compile_jaq_filter(".message | add(") {
            Ok(_) => panic!("the paren is never closed"),
            Err(e) => e.to_string(),
        };

        assert!(said.contains("closing parenthesis"), "{said}");
        assert!(!said.contains("File {"), "no crate internals: {said}");
        assert!(!said.contains("Lex("), "no crate internals: {said}");
    }

    #[test]
    fn a_filter_that_ends_early_says_so() {
        let said = match AssertionEngine::get_or_compile_jaq_filter(".items |") {
            Ok(_) => panic!("the pipe leads nowhere"),
            Err(e) => e.to_string(),
        };

        assert!(said.starts_with("jq expected"), "{said}");
        assert!(said.contains("ended") || said.contains("at `"), "{said}");
    }

    #[test]
    fn a_filter_naming_nothing_says_which_name() {
        let said = match AssertionEngine::get_or_compile_jaq_filter(".a | no_such_filter") {
            Ok(_) => panic!("there is no such filter"),
            Err(e) => e.to_string(),
        };

        assert!(said.contains("no_such_filter"), "{said}");
        assert!(!said.contains("Undefined"), "no crate internals: {said}");
    }

    #[test]
    fn strip_numeric_underscores_merges_digit_separators_outside_strings() {
        assert_eq!(
            strip_numeric_underscores(".amount == 1_000_000"),
            ".amount == 1000000"
        );
        assert_eq!(
            strip_numeric_underscores(".price == 1_234.567_89"),
            ".price == 1234.56789"
        );
        assert_eq!(strip_numeric_underscores(".foo_bar == 1"), ".foo_bar == 1");
        assert_eq!(
            strip_numeric_underscores(r#".id == "a_1_2_3""#),
            r#".id == "a_1_2_3""#
        );
    }

    #[test]
    fn a_conditional_assertion_keeps_the_verdict_it_had_under_jq() {
        let engine = AssertionEngine::new();
        let response = json!({
            "flag": true,
            "off": false,
            "absent": null,
            "name": "Ada",
            "zero": 0
        });

        for (assertion, expected) in [
            ("if .flag then .name else .absent end", true),
            ("if .off then .name else .absent end", false),
            ("if .absent then .name else .name end", true),
            ("if .flag then .zero else .absent end", true),
            ("if .flag == true then .name else .absent end", true),
            ("if .flag then 1 else 2 end", true),
        ] {
            let result = engine.evaluate(assertion, &response, None, None).unwrap();
            assert_eq!(
                result == AssertionResult::Pass,
                expected,
                "{assertion} -> {result:?}"
            );
        }
    }

    #[test]
    fn assertion_with_numeric_digit_separators_matches_the_plain_number() {
        let engine = AssertionEngine::new();
        let response = json!({"amount": 1_000_000});
        let result = engine
            .evaluate(".amount == 1_000_000", &response, None, None)
            .unwrap();
        assert_eq!(result, AssertionResult::Pass);
    }

    #[test]
    fn an_assertion_that_ends_on_its_operator_says_so() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({"name": "Ada", "missing": null});

        let result = engine.evaluate(".name ==", &response, None, None).unwrap();
        match result {
            AssertionResult::Fail { message, .. } => assert!(
                message.contains("ends on `==` with nothing to compare against"),
                "{message}"
            ),
            other => panic!("{other:?}"),
        }

        assert!(matches!(
            engine
                .evaluate(".missing ==", &response, None, None)
                .unwrap(),
            AssertionResult::Fail { .. }
        ));
        assert_eq!(
            engine
                .evaluate(".name == \"Ada\"", &response, None, None)
                .unwrap(),
            AssertionResult::Pass
        );
    }

    #[test]
    fn find_lone_equals_detects_typo() {
        assert_eq!(find_lone_equals(".x = 5"), Some(3));
        assert_eq!(find_lone_equals(".name = \"a\""), Some(6));
    }

    #[test]
    fn find_lone_equals_ignores_comparisons() {
        assert_eq!(find_lone_equals(".x == 5"), None);
        assert_eq!(find_lone_equals(".x != 5"), None);
        assert_eq!(find_lone_equals(".x <= 5"), None);
        assert_eq!(find_lone_equals(".x >= 5"), None);
    }

    #[test]
    fn find_lone_equals_ignores_string_contents() {
        assert_eq!(find_lone_equals(".x == \"a=b\""), None);
        assert_eq!(find_lone_equals(".x == \"a\\\"=b\""), None);
    }

    #[test]
    fn lone_equals_assertion_fails_not_passes() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let result = engine.evaluate(".id = 123", &response, None, None).unwrap();
        assert!(
            matches!(result, AssertionResult::Fail { .. }),
            "lone `=` must fail, got {:?}",
            result
        );
    }

    #[test]
    fn assertion_result_fail() {
        let result = AssertionResult::fail("test message");
        if let AssertionResult::Fail { message, .. } = result {
            assert_eq!(message, "test message");
        } else {
            panic!("Expected Fail result");
        }
    }

    #[test]
    fn assertion_result_fail_with_diff() {
        let result = AssertionResult::fail_with_diff("mismatch", "expected", "actual");
        if let AssertionResult::Fail {
            message,
            expected,
            actual,
            hint: _,
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
    fn assertion_result_debug() {
        let result = AssertionResult::Pass;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Pass"));
    }

    #[test]
    fn evaluate_equality_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id == 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for equality check");
        }
    }

    #[test]
    fn evaluate_bracket_index_assertion() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({
            "ipsToDecorations": {
                "10.0.0.1": {
                    "decoration": "web-frontend",
                    "environment": "production"
                }
            }
        });

        let result1 = engine
            .evaluate(
                ".ipsToDecorations[\"10.0.0.1\"].environment == \"production\"",
                &response,
                None,
                None,
            )
            .unwrap();
        assert!(
            matches!(result1, AssertionResult::Pass),
            "Expected Pass for correct value, got: {:?}",
            result1
        );

        let result2 = engine
            .evaluate(
                ".ipsToDecorations[\"10.0.0.1\"].environment == \"production1\"",
                &response,
                None,
                None,
            )
            .unwrap();
        assert!(
            matches!(result2, AssertionResult::Fail { .. }),
            "Expected Fail for wrong value, got: {:?}",
            result2
        );
    }

    #[test]
    fn evaluate_equality_operator_fail() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id == 456", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { .. } = result {
        } else {
            panic!("Expected Fail for equality check");
        }
    }

    #[test]
    fn evaluate_inequality_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id != 456", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for inequality check");
        }
    }

    #[test]
    fn evaluate_contains_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name contains \"test\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for contains check");
        }
    }

    #[test]
    fn evaluate_contains_operator_array() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags contains \"a\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for array contains check");
        }
    }

    #[test]
    fn evaluate_starts_with_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name startsWith \"te\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for startsWith check");
        }
    }

    #[test]
    fn evaluate_ends_with_operator() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name endsWith \"st\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for endsWith check");
        }
    }

    #[test]
    fn evaluate_numeric_greater_than() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine.evaluate(".id > 100", &response, None, None).unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for greater than check");
        }
    }

    #[test]
    fn evaluate_numeric_less_than() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine.evaluate(".id < 200", &response, None, None).unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for less than check");
        }
    }

    #[test]
    fn evaluate_numeric_gte() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id >= 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for gte check");
        }
    }

    #[test]
    fn evaluate_numeric_lte() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".id <= 123", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for lte check");
        }
    }

    #[test]
    fn evaluate_matches_regex() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name matches \"^te.*t$\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for regex match");
        }
    }

    #[test]
    fn evaluate_matches_regex_fail() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".name matches \"^xyz\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { .. } = result {
        } else {
            panic!("Expected Fail for regex match");
        }
    }

    #[test]
    fn evaluate_nested_path() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".nested.value == 42", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for nested path check");
        }
    }

    #[test]
    fn evaluate_boolean_path() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".active == true", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for boolean check");
        }
    }

    #[test]
    fn evaluate_array_index() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags[0] == \"a\"", &response, None, None)
            .unwrap();
        if let AssertionResult::Pass = result {
        } else {
            panic!("Expected Pass for array index check");
        }
    }

    #[test]
    fn evaluate_unsupported_syntax() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine.evaluate("some_unknown_function()", &response, None, None);
        result.expect("assertion must evaluate");
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
    fn evaluate_all_with_failure() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let assertions = vec![".id == 123".to_string(), ".id == 999".to_string()];

        let results = engine.evaluate_all(&assertions, &response, None, None);
        assert_eq!(results.len(), 2);
        assert!(matches!(&results[0], AssertionResult::Pass));
        assert!(matches!(&results[1], AssertionResult::Fail { .. }));
    }

    #[test]
    fn evaluate_type_cast_number() {
        let engine = AssertionEngine::new();
        let response = json!({
            "price": 42
        });

        let result = engine.evaluate(".price:number >= 0", &response, None, None);
        assert!(
            matches!(result, Ok(AssertionResult::Pass)),
            "Expected Pass, got: {:?}",
            result
        );

        let result = engine.evaluate(".price:number < 0", &response, None, None);
        assert!(
            matches!(result, Ok(AssertionResult::Fail { .. })),
            "Expected Fail, got: {:?}",
            result
        );
    }

    #[test]
    fn evaluate_type_cast_string() {
        let engine = AssertionEngine::new();
        let response = json!({
            "name": "hello world"
        });

        let result = engine.evaluate(".name:string contains \"hello\"", &response, None, None);
        assert!(
            matches!(result, Ok(AssertionResult::Pass)),
            "Expected Pass, got: {:?}",
            result
        );

        let result = engine.evaluate(".name:string startsWith \"he\"", &response, None, None);
        assert!(
            matches!(result, Ok(AssertionResult::Pass)),
            "Expected Pass, got: {:?}",
            result
        );
    }

    #[test]
    fn evaluate_type_cast_is_noop() {
        let engine = AssertionEngine::new();
        let response = json!({
            "value": 123
        });

        let without_cast = engine.evaluate(".value == 123", &response, None, None);
        let with_cast = engine.evaluate(".value:number == 123", &response, None, None);
        assert_eq!(
            matches!(without_cast, Ok(AssertionResult::Pass)),
            matches!(with_cast, Ok(AssertionResult::Pass)),
            "Type cast should not change evaluation result"
        );
    }

    #[test]
    fn jq_fallback_truthy_non_bool_output() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags | length", &response, None, None)
            .unwrap();
        assert!(
            matches!(result, AssertionResult::Pass),
            "Expected Pass, got: {:?}",
            result
        );
    }

    #[test]
    fn jq_fallback_false_output_shows_value() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".tags | length > 10", &response, None, None)
            .unwrap();
        if let AssertionResult::Fail { message, .. } = result {
            assert!(message.contains("false"), "message: {}", message);
        } else {
            panic!("Expected Fail, got: {:?}", result);
        }
    }

    #[test]
    fn jq_fallback_null_output_fails() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let result = engine
            .evaluate(".missing_key | .", &response, None, None)
            .unwrap();
        assert!(
            matches!(result, AssertionResult::Fail { .. }),
            "Expected Fail, got: {:?}",
            result
        );
    }

    #[test]
    fn query_jq_simple() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".id", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(123));
    }

    #[test]
    fn query_jq_nested() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".nested.value", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(42));
    }

    #[test]
    fn query_jq_array() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".tags[]", &response).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], json!("a"));
        assert_eq!(results[1], json!("b"));
        assert_eq!(results[2], json!("c"));
    }

    #[test]
    fn query_jq_filter() {
        let engine = AssertionEngine::new();
        let response = json!([1, 2, 3, 4, 5]);

        let results = engine.query(".[] | select(. > 3)", &response).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], json!(4));
        assert_eq!(results[1], json!(5));
    }

    #[test]
    fn query_jq_length() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query(".tags | length", &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(3));
    }

    #[test]
    fn query_invalid_expression() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        let results = engine.query("invalid[[[", &response);
        assert!(results.is_err());
    }

    #[test]
    fn jaq_to_json_dec_number() {
        let dec = JaqVal::Num(JaqNum::Dec(JaqRc::new("2.5".to_string())));
        assert_eq!(jaq_to_json(&dec), json!(2.5));
    }

    #[test]
    fn jaq_to_json_invalid_dec_number() {
        let dec = JaqVal::Num(JaqNum::Dec(JaqRc::new("not-a-number".to_string())));
        assert_eq!(jaq_to_json(&dec), Value::Null);
    }

    #[test]
    fn json_to_jaq_null() {
        let result = json_to_jaq(&json!(null));
        assert!(matches!(result, JaqVal::Null));
    }

    #[test]
    fn json_to_jaq_bool() {
        let result = json_to_jaq(&json!(true));
        assert!(matches!(result, JaqVal::Bool(true)));
    }

    #[test]
    fn json_to_jaq_number_int() {
        let result = json_to_jaq(&json!(42));
        assert!(matches!(result, JaqVal::Num(JaqNum::Int(42))));
    }

    #[test]
    fn json_to_jaq_number_float() {
        let result = json_to_jaq(&json!(4.14));
        assert!(matches!(result, JaqVal::Num(JaqNum::Float(f)) if (f - 4.14).abs() < 0.001));
    }

    #[test]
    fn json_to_jaq_string() {
        let result = json_to_jaq(&json!("hello"));
        assert!(matches!(result, JaqVal::TStr(_)));
    }

    #[test]
    fn json_to_jaq_array() {
        let result = json_to_jaq(&json!([1, 2, 3]));
        assert!(matches!(result, JaqVal::Arr(_)));
    }

    #[test]
    fn json_to_jaq_object() {
        let result = json_to_jaq(&json!({"key": "value"}));
        assert!(matches!(result, JaqVal::Obj(_)));
    }

    #[test]
    fn jaq_filter_cache_returns_same_arc() {
        let expr = ".__cache_test_sentinel__";
        let first = AssertionEngine::get_or_compile_jaq_filter(expr).unwrap();
        let second = AssertionEngine::get_or_compile_jaq_filter(expr).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }
    #[test]
    fn assertion_result_negate() {
        let pass = AssertionResult::Pass;
        assert!(matches!(pass.negate(), AssertionResult::Fail { .. }));

        let fail = AssertionResult::fail("msg");
        assert!(matches!(fail.negate(), AssertionResult::Pass));

        let error = AssertionResult::Error("err".into());
        assert!(matches!(error.negate(), AssertionResult::Error(_)));
    }

    #[test]
    fn assertion_engine_get_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let assertions = vec![".id == 123".to_string(), ".id == 999".to_string()];
        let results = engine.evaluate_all(&assertions, &response, None, None);
        let failures = engine.get_failures(&results);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn assertion_engine_has_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let result = engine.evaluate_all(&[".id == 999".to_string()], &response, None, None);
        assert!(engine.has_failures(&result));
    }

    #[test]
    fn assertion_engine_no_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let result = engine.evaluate_all(&[".id == 123".to_string()], &response, None, None);
        assert!(!engine.has_failures(&result));
    }

    #[test]
    fn assertion_engine_default() {
        let engine = AssertionEngine::default();
        let response = create_test_response();
        let result = engine
            .evaluate(".id == 123", &response, None, None)
            .unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn assertion_result_fail_with_diff_fields() {
        let result = AssertionResult::fail_with_diff("mismatch", "{\"a\":1}", "{\"a\":2}");
        match result {
            AssertionResult::Fail {
                message,
                expected,
                actual,
                hint: _,
            } => {
                assert_eq!(message, "mismatch");
                assert_eq!(expected.unwrap(), "{\"a\":1}");
                assert_eq!(actual.unwrap(), "{\"a\":2}");
            }
            _ => panic!("Expected Fail"),
        }
    }

    #[test]
    fn evaluate_url_scheme_parse_only() {
        use apif_ast::assertion_ast::{AssertionExpr, assertion_to_string, parse_assertion};
        let expr = parse_assertion("@url.scheme(\"https://example.com\") == \"https\"");
        assert!(
            !matches!(&expr, AssertionExpr::Raw(_)),
            "Expression should be parsed, not Raw: {:?}",
            expr
        );
        let s = assertion_to_string(&expr);
        assert_eq!(
            s, "@url.scheme(\"https://example.com\") == \"https\"",
            "Roundtrip failed"
        );
    }

    #[test]
    fn rewrite_plugin_calls_basic() {
        assert_eq!(
            rewrite_plugin_calls("@len(.items) == .n").unwrap(),
            "__plugin(\"len\"; [.items]) == .n"
        );
    }

    #[test]
    fn rewrite_plugin_calls_multiple_args() {
        assert_eq!(
            rewrite_plugin_calls("@regex(.name, \"^A\")").unwrap(),
            "__plugin(\"regex\"; [.name, \"^A\"])"
        );
    }

    #[test]
    fn rewrite_plugin_calls_nested() {
        assert_eq!(
            rewrite_plugin_calls(".x | map(@is_email(.)) | all").unwrap(),
            ".x | map(__plugin(\"is_email\"; [.])) | all"
        );
    }

    #[test]
    fn rewrite_plugin_calls_leaves_format_strings() {
        assert_eq!(
            rewrite_plugin_calls(".x | @base64").unwrap(),
            ".x | @base64"
        );
    }

    #[test]
    fn rewrite_plugin_calls_ignores_at_in_string() {
        assert_eq!(
            rewrite_plugin_calls(".x == \"@len(a)\"").unwrap(),
            ".x == \"@len(a)\""
        );
    }

    #[test]
    fn rewrite_plugin_calls_rejects_context_plugin() {
        let err = rewrite_plugin_calls("@header(\"x\") | length").unwrap_err();
        assert!(
            err.to_string().contains("not available in jq expressions"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn jaq_context_plugin_reports_clear_error() {
        let engine = AssertionEngine::new();
        let response = json!({"x": 1});
        let result = engine
            .evaluate(".list | map(@header(\"y\")) | all", &response, None, None)
            .unwrap();
        let msg = match result {
            AssertionResult::Error(m) => m,
            AssertionResult::Fail { message, .. } => message,
            other => panic!("expected error/fail, got {:?}", other),
        };
        assert!(
            msg.contains("not available in jq expressions"),
            "unexpected message: {}",
            msg
        );
    }
    #[test]
    fn layered_reports_ast_for_the_fast_path() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({"ok": true});
        let (result, layer) = engine
            .evaluate_with_timing_layered(
                ".ok == true",
                &response,
                None,
                None,
                None,
                &HashMap::new(),
                None,
            )
            .unwrap();
        assert!(matches!(result, AssertionResult::Pass));
        assert_eq!(layer, EvalLayer::Ast);
    }

    #[test]
    fn layered_reports_jq_for_the_fallback() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({"items": [1, 2]});
        let (result, layer) = engine
            .evaluate_with_timing_layered(
                ".items | map(. * 2) | add == 6",
                &response,
                None,
                None,
                None,
                &HashMap::new(),
                None,
            )
            .unwrap();
        assert!(matches!(result, AssertionResult::Pass), "{result:?}");
        assert_eq!(layer, EvalLayer::Jq);
    }

    #[test]
    fn layered_variables_resolve_only_in_ast() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({"name": "ada"});
        let mut vars = HashMap::new();
        vars.insert("expected".to_string(), serde_json::json!("ada"));
        let (result, layer) = engine
            .evaluate_with_timing_layered(
                ".name == $expected",
                &response,
                None,
                None,
                None,
                &vars,
                None,
            )
            .unwrap();
        assert!(matches!(result, AssertionResult::Pass), "{result:?}");
        assert_eq!(layer, EvalLayer::Ast);
    }

    #[test]
    fn query_bounded_stops_a_generator_that_never_ends() {
        let engine = AssertionEngine::new();
        let started = std::time::Instant::now();
        let err = engine
            .query_bounded(
                "repeat(.)",
                &serde_json::json!({}),
                10_000,
                std::time::Duration::from_millis(200),
            )
            .unwrap_err();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "must not spin"
        );
        let msg = err.to_string();
        assert!(msg.contains("outputs") || msg.contains("in time"), "{msg}");
    }

    #[test]
    fn query_bounded_returns_normal_results_untouched() {
        let engine = AssertionEngine::new();
        let out = engine
            .query_bounded(
                ".items[].id",
                &serde_json::json!({"items": [{"id": 1}, {"id": 2}]}),
                1000,
                std::time::Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(out, vec![serde_json::json!(1), serde_json::json!(2)]);
    }

    #[test]
    fn evaluate_bounded_turns_a_runaway_into_an_error_verdict() {
        let engine = AssertionEngine::new();
        let (result, layer) = engine
            .evaluate_bounded(
                "repeat(.)",
                &serde_json::json!({}),
                None,
                None,
                None,
                &HashMap::new(),
                None,
                10_000,
                std::time::Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(layer, EvalLayer::Jq);
        assert!(matches!(result, AssertionResult::Error(_)), "{result:?}");
    }

    #[test]
    fn the_jq_env_builtin_is_not_compiled() {
        let engine = AssertionEngine::new();
        let err = engine
            .query("env", &serde_json::json!({}))
            .expect_err("env must not compile");
        assert!(err.to_string().contains("env"), "{err}");

        assert!(engine.query("env.HOME", &serde_json::json!({})).is_err());
        assert!(engine.query("$ENV.HOME", &serde_json::json!({})).is_err());
    }

    #[test]
    fn removing_env_leaves_the_rest_of_jq_std_intact() {
        let engine = AssertionEngine::new();
        let out = engine
            .query(
                "[.[] | ascii_downcase] | join(\",\")",
                &serde_json::json!(["A", "B"]),
            )
            .unwrap();
        assert_eq!(out, vec![serde_json::json!("a,b")]);
    }

    #[test]
    fn exactly_max_outputs_is_not_a_runaway() {
        let engine = AssertionEngine::new();
        let input = serde_json::json!((0..10).collect::<Vec<_>>());
        let out = engine
            .query_bounded(".[]", &input, 10, std::time::Duration::from_secs(1))
            .expect("ten outputs under a limit of ten must be allowed");
        assert_eq!(out.len(), 10);

        let err = engine
            .query_bounded(
                ".[]",
                &serde_json::json!((0..11).collect::<Vec<_>>()),
                10,
                std::time::Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(err.to_string().contains("more than 10"), "{err}");
    }
}
