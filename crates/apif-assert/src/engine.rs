// Assertion engine using embedded jaq and operators fallback

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

/// Thread-safe cache for compiled JQ filters.
/// Uses `Mutex` instead of `thread_local!` + `RefCell` to be safe with
/// tokio's work-stealing runtime where futures can migrate across threads.
static JAQ_FILTER_CACHE: LazyLock<Mutex<HashMap<String, Arc<JaqFilter>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Plugins that depend on external context (headers, trailers, timing, env) and
/// therefore cannot be a pure function of a single JSON value. They stay
/// AST-engine-only and are rejected with a clear message if used inside a jq
/// expression (the jaq-fallback path).
const JAQ_CONTEXT_ONLY_PLUGINS: &[&str] = &[
    "header",
    "has_header",
    "trailer",
    "has_trailer",
    "elapsed_ms",
    "total_elapsed_ms",
    "env",
    "scope.message_count",
    "scope.index",
    "scope_message_count",
    "scope_index",
];

thread_local! {
    /// Registry made available to the `__plugin` native jaq function for the
    /// duration of a single `run_jaq` call. jaq native filters are bare `fn`
    /// pointers and cannot capture state, so we hand the registry over via a
    /// thread-local that is set (and restored) by [`PluginRegistryGuard`].
    static JAQ_PLUGIN_REGISTRY: RefCell<Option<Arc<dyn crate::registry::PluginRegistry>>> =
        const { RefCell::new(None) };
}

/// RAII guard that installs the plugin registry into the thread-local for the
/// current jaq run and restores the previous value on drop (reentrancy-safe).
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

/// Look the plugin up in the thread-local registry, execute it against `args`,
/// and map its result into a jaq value: a `PluginResult::Value(v)` becomes `v`,
/// and a passing/failing assertion becomes `true`/`false`, so plugins compose
/// with jq operators (`map`, `select`, `all`, arithmetic).
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

/// The `__plugin` native function registered into every compiled jaq filter.
///
/// Invoked as `__plugin("name"; [arg, ...])` — the form produced by
/// [`rewrite_plugin_calls`] from `@name(arg, ...)`. It evaluates the name and
/// argument filters against the current input, then dispatches to the plugin.
///
/// Written as a closure (not a named fn) so it coerces cleanly to jaq's
/// higher-ranked `RunPtr<D>`, mirroring how `jaq_json` defines native filters.
fn jaq_plugin_fun<D>() -> jaq_core::native::Fun<D>
where
    D: for<'a> jaq_core::DataT<V<'a> = JaqVal>,
{
    jaq_core::native::run((
        "__plugin",
        Box::new([Bind::Fun(()), Bind::Fun(())]),
        |mut cv: Cv<D>| {
            let input = cv.1.clone();
            // Arguments are popped last-to-first: `__plugin(name; args)`.
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

/// Rewrite `@name(args)` plugin calls into `__plugin("name"; [args])` so jaq can
/// dispatch them to registered plugins. Nested plugin calls and string literals
/// are handled; jq format strings like `@base64` (not followed by `(`) are left
/// untouched. Context-dependent plugins are rejected with a clear message.
fn rewrite_plugin_calls(expr: &str) -> Result<String> {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' | b'\'' => {
                // Copy the whole string literal verbatim.
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

/// Length in bytes of a UTF-8 sequence starting with the leading byte `b`.
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

/// Given the index of an opening `(`, return the index of its matching `)`,
/// tracking nested brackets and skipping string literals.
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
    /// Create a new assertion engine with default plugins
    pub fn new() -> Self {
        Self {
            plugin_registry: Arc::new(crate::registry::NoopPluginRegistry),
        }
    }

    /// Create a new assertion engine with a custom plugin registry
    pub fn with_registry(registry: Arc<dyn crate::registry::PluginRegistry>) -> Self {
        Self {
            plugin_registry: registry,
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
        self.evaluate_with_timing(
            assertion,
            response,
            headers,
            trailers,
            None,
            &HashMap::new(),
        )
    }

    pub fn evaluate_with_timing(
        &self,
        assertion: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
    ) -> Result<AssertionResult> {
        let trimmed = assertion.trim();

        match operators::evaluate_assertion(
            &*self.plugin_registry,
            trimmed,
            response,
            headers,
            trailers,
            timing,
            variables,
        ) {
            Ok(Some(result)) => Ok(result),
            Ok(None) => {
                // AST could not parse it — fall through to JQ.
                // A lone `=` (not `==`/`!=`/`<=`/`>=`) reaching this point is almost
                // always a typo for `==`; jq would silently treat it as assignment
                // (truthy) and the assertion would false-pass. Reject it explicitly.
                if let Some(pos) = find_lone_equals(trimmed) {
                    return Ok(AssertionResult::fail(format!(
                        "Assertion uses `=` at position {} — did you mean `==`? \
                         (`=` is not a comparison operator): {}",
                        pos, trimmed
                    )));
                }
                self.evaluate_jaq(trimmed, response)
            }
            Err(e) => Err(e),
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

        // JQ truthiness: everything except `false` and `null` is truthy
        // (so e.g. `.tags | length` returning 3 passes).
        for val in &out {
            if matches!(val, JaqVal::Bool(false) | JaqVal::Null) {
                let rendered = serde_json::to_string(&jaq_to_json(val))
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                return Ok(AssertionResult::fail(format!(
                    "JQ assertion evaluated to falsy value {}: {}",
                    rendered, expr
                )));
            }
        }

        if out.is_empty() {
            Ok(AssertionResult::fail(format!(
                "JQ assertion produced no output (falsey): {}",
                expr
            )))
        } else {
            Ok(AssertionResult::Pass)
        }
    }

    fn run_jaq(&self, expr: &str, input: &Value) -> Result<Vec<JaqVal>> {
        // Rewrite `@plugin(...)` calls so jaq can dispatch them to registered
        // plugins; the cache is keyed on the rewritten form for consistency.
        let rewritten = rewrite_plugin_calls(expr)?;
        let filter = Self::get_or_compile_jaq_filter(&rewritten)?;

        let input = json_to_jaq(input);

        // Expose plugins to the `__plugin` native function for this run only.
        let _registry_guard = PluginRegistryGuard::set(self.plugin_registry.clone());

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

        let arena = load::Arena::default();
        let defs = core_defs().chain(jaq_std::defs()).chain(jaq_json::defs());
        let funs = core_funs()
            .chain(jaq_std::funs())
            .chain(jaq_json::funs())
            .chain(std::iter::once(jaq_plugin_fun()));
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

        let filter = Arc::new(filter);
        JAQ_FILTER_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(expr.to_string(), Arc::clone(&filter));

        Ok(filter)
    }

    /// Evaluate a JQ expression against `input`, returning the first output value.
    /// Uses `JAQ_FILTER_CACHE` to avoid recompilation on repeated calls.
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

    // Check if any assertion failed (re-exported wrapper)
    #[must_use]
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
        self.evaluate_all_with_timing(
            assertions,
            response,
            headers,
            trailers,
            None,
            &HashMap::new(),
        )
    }

    pub fn evaluate_all_with_timing(
        &self,
        assertions: &[String],
        response: &serde_json::Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
        timing: Option<&AssertionTiming>,
        variables: &HashMap<String, Value>,
    ) -> Vec<AssertionResult> {
        assertions
            .iter()
            .map(|assertion| {
                self.evaluate_with_timing(assertion, response, headers, trailers, timing, variables)
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

/// Find a top-level lone `=` (not part of `==`, `!=`, `<=`, `>=`) outside of
/// string literals. Returns the byte position of the offending `=`, if any.
/// Used to catch `.x = 5` typos before they reach jq (where `=` is assignment).
fn find_lone_equals(expr: &str) -> Option<usize> {
    let bytes = expr.as_bytes();
    let mut in_string: Option<u8> = None; // Some(quote_char) while inside a string
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match in_string {
            Some(q) => {
                if b == b'\\' {
                    i += 2; // skip escaped char
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
                    // Skip `==`, and the second `=` of `!=`/`<=`/`>=`/`==`.
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
    fn test_find_lone_equals_detects_typo() {
        assert_eq!(find_lone_equals(".x = 5"), Some(3));
        assert_eq!(find_lone_equals(".name = \"a\""), Some(6));
    }

    #[test]
    fn test_find_lone_equals_ignores_comparisons() {
        assert_eq!(find_lone_equals(".x == 5"), None);
        assert_eq!(find_lone_equals(".x != 5"), None);
        assert_eq!(find_lone_equals(".x <= 5"), None);
        assert_eq!(find_lone_equals(".x >= 5"), None);
    }

    #[test]
    fn test_find_lone_equals_ignores_string_contents() {
        // `=` inside a string literal is not a typo'd operator
        assert_eq!(find_lone_equals(".x == \"a=b\""), None);
        assert_eq!(find_lone_equals(".x == \"a\\\"=b\""), None);
    }

    #[test]
    fn test_lone_equals_assertion_fails_not_passes() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        // `.id = 123` is a typo for `==`; must be a diagnosed failure, not a
        // silent jq-assignment pass.
        let result = engine.evaluate(".id = 123", &response, None, None).unwrap();
        assert!(
            matches!(result, AssertionResult::Fail { .. }),
            "lone `=` must fail, got {:?}",
            result
        );
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
    fn test_evaluate_bracket_index_assertion() {
        let engine = AssertionEngine::new();
        let response = serde_json::json!({
            "ipsToDecorations": {
                "10.0.0.1": {
                    "decoration": "web-frontend",
                    "environment": "production"
                }
            }
        });

        // Correct value - should PASS
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

        // Wrong value - should FAIL
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
    fn test_evaluate_type_cast_number() {
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
    fn test_evaluate_type_cast_string() {
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
    fn test_evaluate_type_cast_is_noop() {
        let engine = AssertionEngine::new();
        let response = json!({
            "value": 123
        });

        // Type cast should not affect evaluation result
        let without_cast = engine.evaluate(".value == 123", &response, None, None);
        let with_cast = engine.evaluate(".value:number == 123", &response, None, None);
        assert_eq!(
            matches!(without_cast, Ok(AssertionResult::Pass)),
            matches!(with_cast, Ok(AssertionResult::Pass)),
            "Type cast should not change evaluation result"
        );
    }

    #[test]
    fn test_jq_fallback_truthy_non_bool_output() {
        // Regression: jq truthiness — any output except false/null passes,
        // so `.tags | length` returning 3 must be a Pass.
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
    fn test_jq_fallback_false_output_shows_value() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        // `.tags | length > 10` is 3 > 10 == false — must fail and show the value
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
    fn test_jq_fallback_null_output_fails() {
        let engine = AssertionEngine::new();
        let response = create_test_response();

        // Missing key piped through identity yields null — falsy in jq
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

    #[test]
    fn test_json_to_jaq_null() {
        let result = json_to_jaq(&json!(null));
        assert!(matches!(result, JaqVal::Null));
    }

    #[test]
    fn test_json_to_jaq_bool() {
        let result = json_to_jaq(&json!(true));
        assert!(matches!(result, JaqVal::Bool(true)));
    }

    #[test]
    fn test_json_to_jaq_number_int() {
        let result = json_to_jaq(&json!(42));
        assert!(matches!(result, JaqVal::Num(JaqNum::Int(42))));
    }

    #[test]
    fn test_json_to_jaq_number_float() {
        let result = json_to_jaq(&json!(4.14));
        assert!(matches!(result, JaqVal::Num(JaqNum::Float(f)) if (f - 4.14).abs() < 0.001));
    }

    #[test]
    fn test_json_to_jaq_string() {
        let result = json_to_jaq(&json!("hello"));
        assert!(matches!(result, JaqVal::TStr(_)));
    }

    #[test]
    fn test_json_to_jaq_array() {
        let result = json_to_jaq(&json!([1, 2, 3]));
        assert!(matches!(result, JaqVal::Arr(_)));
    }

    #[test]
    fn test_json_to_jaq_object() {
        let result = json_to_jaq(&json!({"key": "value"}));
        assert!(matches!(result, JaqVal::Obj(_)));
    }

    #[test]
    fn test_jaq_filter_cache_returns_same_arc() {
        let expr = ".__cache_test_sentinel__";
        let first = AssertionEngine::get_or_compile_jaq_filter(expr).unwrap();
        let second = AssertionEngine::get_or_compile_jaq_filter(expr).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }
    #[test]
    fn test_assertion_result_negate() {
        let pass = AssertionResult::Pass;
        assert!(matches!(pass.negate(), AssertionResult::Fail { .. }));

        let fail = AssertionResult::fail("msg");
        assert!(matches!(fail.negate(), AssertionResult::Pass));

        let error = AssertionResult::Error("err".into());
        assert!(matches!(error.negate(), AssertionResult::Error(_)));
    }

    #[test]
    fn test_assertion_engine_get_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let assertions = vec![".id == 123".to_string(), ".id == 999".to_string()];
        let results = engine.evaluate_all(&assertions, &response, None, None);
        let failures = engine.get_failures(&results);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn test_assertion_engine_has_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let result = engine.evaluate_all(&[".id == 999".to_string()], &response, None, None);
        assert!(engine.has_failures(&result));
    }

    #[test]
    fn test_assertion_engine_no_failures() {
        let engine = AssertionEngine::new();
        let response = create_test_response();
        let result = engine.evaluate_all(&[".id == 123".to_string()], &response, None, None);
        assert!(!engine.has_failures(&result));
    }

    #[test]
    fn test_assertion_engine_default() {
        let engine = AssertionEngine::default();
        let response = create_test_response();
        let result = engine
            .evaluate(".id == 123", &response, None, None)
            .unwrap();
        assert!(matches!(result, AssertionResult::Pass));
    }

    #[test]
    fn test_assertion_result_fail_with_diff_fields() {
        let result = AssertionResult::fail_with_diff("mismatch", "{\"a\":1}", "{\"a\":2}");
        match result {
            AssertionResult::Fail {
                message,
                expected,
                actual,
            } => {
                assert_eq!(message, "mismatch");
                assert_eq!(expected.unwrap(), "{\"a\":1}");
                assert_eq!(actual.unwrap(), "{\"a\":2}");
            }
            _ => panic!("Expected Fail"),
        }
    }

    #[test]
    fn test_evaluate_url_scheme_parse_only() {
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
    fn test_rewrite_plugin_calls_basic() {
        assert_eq!(
            rewrite_plugin_calls("@len(.items) == .n").unwrap(),
            "__plugin(\"len\"; [.items]) == .n"
        );
    }

    #[test]
    fn test_rewrite_plugin_calls_multiple_args() {
        assert_eq!(
            rewrite_plugin_calls("@regex(.name, \"^A\")").unwrap(),
            "__plugin(\"regex\"; [.name, \"^A\"])"
        );
    }

    #[test]
    fn test_rewrite_plugin_calls_nested() {
        assert_eq!(
            rewrite_plugin_calls(".x | map(@is_email(.)) | all").unwrap(),
            ".x | map(__plugin(\"is_email\"; [.])) | all"
        );
    }

    #[test]
    fn test_rewrite_plugin_calls_leaves_format_strings() {
        // `@base64` is a jq format string (not followed by `(`) — must be untouched.
        assert_eq!(
            rewrite_plugin_calls(".x | @base64").unwrap(),
            ".x | @base64"
        );
    }

    #[test]
    fn test_rewrite_plugin_calls_ignores_at_in_string() {
        // A `@name(` sequence inside a string literal is not a plugin call.
        assert_eq!(
            rewrite_plugin_calls(".x == \"@len(a)\"").unwrap(),
            ".x == \"@len(a)\""
        );
    }

    #[test]
    fn test_rewrite_plugin_calls_rejects_context_plugin() {
        let err = rewrite_plugin_calls("@header(\"x\") | length").unwrap_err();
        assert!(
            err.to_string().contains("not available in jq expressions"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_jaq_context_plugin_reports_clear_error() {
        // A context-dependent plugin used inside a jq pipe (so the AST engine can't
        // handle it and it falls to jaq) must yield a clear message, not a parse error.
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
}
