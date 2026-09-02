use std::collections::HashMap;
use std::time::Duration;

use axum::Json;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::assert::{AssertionEngine, AssertionResult, EvalLayer};

const EVAL_TIMEOUT: Duration = Duration::from_secs(3);

const ENGINE_BUDGET: Duration = Duration::from_millis(1500);

const MAX_OUTPUTS: usize = 10_000;

const MAX_EXPRESSIONS: usize = 200;

const EVAL_THREADS: usize = 4;

static EVAL_PERMITS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(EVAL_THREADS)));

static ORPHANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

static PLUGIN_REGISTRY: std::sync::LazyLock<std::sync::Arc<dyn crate::assert::PluginRegistry>> =
    std::sync::LazyLock::new(|| {
        std::sync::Arc::new(crate::execution::plugin_dir::build_plugin_manager())
    });

async fn bounded<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, (StatusCode, String)> {
    bounded_with(EVAL_PERMITS.clone(), work).await
}

async fn bounded_with<T: Send + 'static>(
    permits: std::sync::Arc<tokio::sync::Semaphore>,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, (StatusCode, String)> {
    if ORPHANS.load(std::sync::atomic::Ordering::Relaxed) >= EVAL_THREADS {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "{} earlier evaluations are still burning CPU and cannot be cancelled — \
                 an expression like `[range(1e11)] | length` produces one value and never \
                 yields, so nothing can interrupt it. Restart `grpctestify play` to clear them.",
                EVAL_THREADS
            ),
        ));
    }

    let Ok(Ok(permit)) =
        tokio::time::timeout(Duration::from_millis(750), permits.acquire_owned()).await
    else {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "the playground evaluator is saturated — earlier expressions are still running; \
             try again in a moment"
                .to_string(),
        ));
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    let orphaned = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_flag = orphaned.clone();
    std::thread::spawn(move || {
        let out = work();
        if thread_flag.load(std::sync::atomic::Ordering::Relaxed) {
            ORPHANS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = tx.send(out);
    });

    let outcome = tokio::time::timeout(EVAL_TIMEOUT, rx).await;
    match outcome {
        Ok(Ok(v)) => {
            drop(permit);
            Ok(v)
        }
        Ok(Err(e)) => {
            drop(permit);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        Err(_) => {
            orphaned.store(true, std::sync::atomic::Ordering::Relaxed);
            ORPHANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            drop(permit);
            Err((
                StatusCode::GATEWAY_TIMEOUT,
                format!(
                    "evaluation exceeded {}s — jq programs like `repeat`/`until` can loop forever",
                    EVAL_TIMEOUT.as_secs()
                ),
            ))
        }
    }
}

#[derive(Deserialize)]
pub struct EvalAssertRequest {
    pub response: Value,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub trailers: HashMap<String, String>,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub variables: HashMap<String, Value>,
    pub expressions: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct EvalAssertVerdict {
    pub expression: String,
    pub layer: Option<String>,
    pub suggestion: Option<String>,
    pub passed: bool,
    pub error: Option<String>,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub elapsed_us: u64,
}

pub async fn eval_assert(
    Json(req): Json<EvalAssertRequest>,
) -> Result<Json<Vec<EvalAssertVerdict>>, (StatusCode, String)> {
    if req.expressions.len() > MAX_EXPRESSIONS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "{} expressions in one request — the playground evaluates at most {MAX_EXPRESSIONS} at a time",
                req.expressions.len()
            ),
        ));
    }
    let _ = tokio::task::spawn_blocking(|| PLUGIN_REGISTRY.clone()).await;
    let verdicts = bounded(move || {
        let engine = AssertionEngine::with_registry(PLUGIN_REGISTRY.clone());
        let timing = crate::assert::AssertionTiming {
            elapsed_ms: req.elapsed_ms,
            total_elapsed_ms: req.elapsed_ms,
            scope_message_count: 1,
            scope_index: 0,
        };

        req.expressions
            .iter()
            .map(|expr| {
                let trimmed = expr.trim();
                if trimmed.is_empty() {
                    return EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: None,
                        suggestion: suggest(trimmed),
                        passed: false,
                        error: Some("empty expression".to_string()),
                        message: None,
                        expected: None,
                        actual: None,
                        hint: None,
                        elapsed_us: 0,
                    };
                }
                if trimmed.contains("@env") {
                    return EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: None,
                        suggestion: suggest(trimmed),
                        passed: false,
                        error: Some(
                            "@env is not available in the playground — it reads the server's \
                             whole process environment"
                                .to_string(),
                        ),
                        message: None,
                        expected: None,
                        actual: None,
                        hint: None,
                        elapsed_us: 0,
                    };
                }

                let started = std::time::Instant::now();
                let outcome = engine.evaluate_bounded(
                    trimmed,
                    &req.response,
                    Some(&req.headers),
                    Some(&req.trailers),
                    Some(&timing),
                    &req.variables,
                    None,
                    MAX_OUTPUTS,
                    ENGINE_BUDGET,
                );
                let elapsed_us = started.elapsed().as_micros() as u64;

                match outcome {
                    Ok((AssertionResult::Pass, layer)) => EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: Some(layer_name(layer)),
                        suggestion: suggest(trimmed),
                        passed: true,
                        error: None,
                        message: None,
                        expected: None,
                        actual: None,
                        hint: None,
                        elapsed_us,
                    },
                    Ok((
                        AssertionResult::Fail {
                            message,
                            expected,
                            actual,
                            hint,
                        },
                        layer,
                    )) => EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: Some(layer_name(layer)),
                        suggestion: suggest(trimmed),
                        passed: false,
                        error: None,
                        message: Some(message),
                        expected,
                        actual,
                        hint,
                        elapsed_us,
                    },
                    Ok((AssertionResult::Error(e), layer)) => EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: Some(layer_name(layer)),
                        suggestion: suggest(trimmed),
                        passed: false,
                        error: Some(e),
                        message: None,
                        expected: None,
                        actual: None,
                        hint: None,
                        elapsed_us,
                    },
                    Err(e) => EvalAssertVerdict {
                        expression: expr.clone(),
                        layer: None,
                        suggestion: suggest(trimmed),
                        passed: false,
                        error: Some(e.to_string()),
                        message: None,
                        expected: None,
                        actual: None,
                        hint: None,
                        elapsed_us,
                    },
                }
            })
            .collect::<Vec<_>>()
    })
    .await?;
    Ok(Json(verdicts))
}

fn suggest(expr: &str) -> Option<String> {
    apif_optimizer::rewrite_assertion_expression_fixed_point_if_changed(expr)
}

fn layer_name(layer: EvalLayer) -> String {
    match layer {
        EvalLayer::Ast => "ast".to_string(),
        EvalLayer::Jq => "jq".to_string(),
    }
}

#[derive(Deserialize)]
pub struct EvalQueryRequest {
    pub input: Value,
    pub expr: String,
}

#[derive(Serialize)]
pub struct EvalQueryResponse {
    pub outputs: Vec<Value>,
    pub error: Option<String>,
    pub elapsed_us: u64,
}

pub async fn eval_query(
    Json(req): Json<EvalQueryRequest>,
) -> Result<Json<EvalQueryResponse>, (StatusCode, String)> {
    if req.expr.contains("@env") {
        return Ok(Json(EvalQueryResponse {
            outputs: vec![],
            error: Some("@env is not available in the playground".to_string()),
            elapsed_us: 0,
        }));
    }
    let out = bounded(move || {
        let engine = AssertionEngine::with_registry(PLUGIN_REGISTRY.clone());
        let started = std::time::Instant::now();
        let result = engine.query_bounded(&req.expr, &req.input, MAX_OUTPUTS, ENGINE_BUDGET);
        let elapsed_us = started.elapsed().as_micros() as u64;
        match result {
            Ok(outputs) => EvalQueryResponse {
                outputs,
                error: None,
                elapsed_us,
            },
            Err(e) => EvalQueryResponse {
                outputs: vec![],
                error: Some(e.to_string()),
                elapsed_us,
            },
        }
    })
    .await?;
    Ok(Json(out))
}

fn utf16_len(chars: &[char]) -> usize {
    chars.iter().map(|c| c.len_utf16()).sum()
}

fn utf16_len_str(text: &str) -> usize {
    text.chars().map(|c| c.len_utf16()).sum()
}

#[derive(Deserialize)]
pub struct EvalRegexRequest {
    pub pattern: String,
    #[serde(default)]
    pub flags: String,
    pub subject: String,
    pub mode: String,
}

#[derive(Serialize)]
pub struct EvalRegexResponse {
    pub rewritten_pattern: String,
    pub matched: bool,
    pub spans: Vec<(usize, usize)>,
    pub captures: Vec<(String, String)>,
    pub error: Option<String>,
}

pub async fn eval_regex(
    Json(req): Json<EvalRegexRequest>,
) -> Result<Json<EvalRegexResponse>, (StatusCode, String)> {
    let out = bounded(move || {
        let rewritten = crate::assert::regex_with_flags(&req.pattern, &req.flags);
        match req.mode.as_str() {
            "jq" => {
                let engine = AssertionEngine::with_registry(PLUGIN_REGISTRY.clone());
                let input = Value::String(req.subject.clone());
                let expr = format!(
                    "[match({}; {})]",
                    Value::String(req.pattern.clone()),
                    Value::String(req.flags.clone()),
                );
                match engine.query_bounded(&expr, &input, MAX_OUTPUTS, ENGINE_BUDGET) {
                    Ok(outputs) => {
                        let matches = outputs
                            .first()
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let chars: Vec<char> = req.subject.chars().collect();
                        let spans = matches
                            .iter()
                            .filter_map(|m| {
                                let offset = m.get("offset")?.as_u64()? as usize;
                                let length = m.get("length")?.as_u64()? as usize;
                                Some((
                                    utf16_len(&chars[..offset.min(chars.len())]),
                                    utf16_len(&chars[..(offset + length).min(chars.len())]),
                                ))
                            })
                            .collect::<Vec<_>>();
                        let captures = matches
                            .first()
                            .and_then(|m| m.get("captures"))
                            .and_then(|c| c.as_array())
                            .map(|caps| {
                                caps.iter()
                                    .enumerate()
                                    .filter_map(|(i, c)| {
                                        let text = c.get("string")?.as_str()?.to_string();
                                        let name = c
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .map(str::to_string)
                                            .unwrap_or_else(|| (i + 1).to_string());
                                        Some((name, text))
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        EvalRegexResponse {
                            rewritten_pattern: rewritten,
                            matched: !matches.is_empty(),
                            spans,
                            captures,
                            error: None,
                        }
                    }
                    Err(e) => EvalRegexResponse {
                        rewritten_pattern: rewritten,
                        matched: false,
                        spans: vec![],
                        captures: vec![],
                        error: Some(e.to_string()),
                    },
                }
            }
            _ => match crate::assert::cached_regex(&rewritten) {
                Ok(re) => {
                    let spans = re
                        .find_iter(&req.subject)
                        .map(|m| {
                            (
                                utf16_len_str(&req.subject[..m.start()]),
                                utf16_len_str(&req.subject[..m.end()]),
                            )
                        })
                        .collect::<Vec<_>>();
                    EvalRegexResponse {
                        rewritten_pattern: rewritten,
                        matched: !spans.is_empty(),
                        spans,
                        captures: vec![],
                        error: None,
                    }
                }
                Err(e) => EvalRegexResponse {
                    rewritten_pattern: rewritten,
                    matched: false,
                    spans: vec![],
                    captures: vec![],
                    error: Some(e),
                },
            },
        }
    })
    .await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn assert_one(req: EvalAssertRequest) -> EvalAssertVerdict {
        eval_assert(Json(req))
            .await
            .unwrap()
            .0
            .into_iter()
            .next()
            .unwrap()
    }

    fn req(response: Value, expr: &str) -> EvalAssertRequest {
        EvalAssertRequest {
            response,
            headers: HashMap::new(),
            trailers: HashMap::new(),
            elapsed_ms: 42,
            variables: HashMap::new(),
            expressions: vec![expr.to_string()],
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_passing_assertion_names_its_layer() {
        let v = assert_one(req(json!({"ok": true}), ".ok == true")).await;
        assert!(v.passed);
        assert_eq!(v.layer.as_deref(), Some("ast"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_jq_fallback_is_labelled_jq() {
        let v = assert_one(req(
            json!({"items": [1, 2]}),
            ".items | map(. * 2) | add == 6",
        ))
        .await;
        assert!(v.passed, "{v:?}");
        assert_eq!(v.layer.as_deref(), Some("jq"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_failure_carries_expected_and_actual() {
        let v = assert_one(req(json!({"n": 2}), ".n == 3")).await;
        assert!(!v.passed);
        assert!(v.expected.is_some() || v.message.is_some());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn env_is_refused_not_evaluated() {
        let v = assert_one(req(json!({}), "@env(\"HOME\") != \"\"")).await;
        assert!(!v.passed);
        assert!(v.error.unwrap().contains("@env"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn context_plugins_get_a_verdict_not_an_error() {
        let v = assert_one(req(json!({}), "@elapsed_ms() < 1000")).await;
        assert!(v.passed, "{v:?}");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_verdict_carries_what_fmt_would_rewrite_the_line_to() {
        let verdicts = eval_assert(Json(EvalAssertRequest {
            response: serde_json::json!({"items": []}),
            headers: Default::default(),
            trailers: Default::default(),
            elapsed_ms: 0,
            variables: Default::default(),
            expressions: vec!["@len(.items) == 0".to_string(), ".ok == true".to_string()],
        }))
        .await
        .expect("evaluates")
        .0;

        assert_eq!(
            verdicts[0].suggestion.as_deref(),
            Some("@is_empty(.items)"),
            "the optimizer's own rewrite, so fmt cannot undo the suggestion"
        );
        assert!(
            verdicts[1].suggestion.is_none(),
            "a line the optimizer leaves alone suggests nothing"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_runaway_jq_program_times_out_instead_of_hanging() {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let out = bounded_with(sem, || {
            let engine = crate::assert::AssertionEngine::new();
            engine.query("repeat(.)", &json!({}))
        })
        .await;
        match out {
            Err((code, msg)) => {
                assert_eq!(code, StatusCode::GATEWAY_TIMEOUT);
                assert!(msg.contains("exceeded"));
            }
            Ok(_) => panic!("repeat(.) must not terminate"),
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn the_endpoint_stops_a_runaway_in_the_engine_not_by_abandoning_it() {
        let started = std::time::Instant::now();
        let out = eval_query(Json(EvalQueryRequest {
            input: json!({}),
            expr: "repeat(.)".to_string(),
        }))
        .await
        .expect("must not time out at the request layer")
        .0;
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "engine budget must bite first"
        );
        assert!(out.outputs.is_empty());
        assert!(out.error.is_some());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_runaway_assertion_is_an_error_verdict_not_a_hang() {
        let v = assert_one(req(json!({}), "repeat(.)")).await;
        assert!(!v.passed);
        assert!(v.error.is_some());
        assert_eq!(v.layer.as_deref(), Some("jq"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn regex_matches_mode_rewrites_flags_and_reports_spans() {
        let out = eval_regex(Json(EvalRegexRequest {
            pattern: "wor.d?".to_string(),
            flags: "gi".to_string(),
            subject: "Hello WORLD".to_string(),
            mode: "matches".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert_eq!(out.rewritten_pattern, "(?i)wor.d?");
        assert!(out.matched);
        assert_eq!(out.spans, vec![(6, 11)]);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn regex_jq_mode_returns_named_captures() {
        let out = eval_regex(Json(EvalRegexRequest {
            pattern: "(?<major>\\d+)\\.(?<minor>\\d+)".to_string(),
            flags: String::new(),
            subject: "v1.42".to_string(),
            mode: "jq".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert!(out.matched, "{:?}", out.error);
        assert_eq!(
            out.captures,
            vec![
                ("major".to_string(), "1".to_string()),
                ("minor".to_string(), "42".to_string())
            ]
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn an_invalid_regex_is_an_error_not_a_500() {
        let out = eval_regex(Json(EvalRegexRequest {
            pattern: "(".to_string(),
            flags: String::new(),
            subject: "x".to_string(),
            mode: "matches".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert!(!out.matched);
        assert!(out.error.is_some());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn query_returns_every_output() {
        let out = eval_query(Json(EvalQueryRequest {
            input: json!({"items": [{"id": 1}, {"id": 2}]}),
            expr: ".items[].id".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert_eq!(out.outputs, vec![json!(1), json!(2)]);
    }
    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_timed_out_evaluation_gives_its_permit_back() {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let before = ORPHANS.load(std::sync::atomic::Ordering::Relaxed);

        let out = bounded_with(sem.clone(), || {
            let engine = crate::assert::AssertionEngine::new();
            engine.query("repeat(.)", &json!({}))
        })
        .await;
        assert!(matches!(out, Err((StatusCode::GATEWAY_TIMEOUT, _))));
        assert_eq!(sem.available_permits(), 1, "the permit must return");
        assert!(
            ORPHANS.load(std::sync::atomic::Ordering::Relaxed) > before,
            "the abandoned thread must be counted"
        );

        ORPHANS.store(0, std::sync::atomic::Ordering::Relaxed);
        let next = bounded_with(sem, || 7).await;
        assert_eq!(next.unwrap(), 7);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn a_saturated_evaluator_refuses_instead_of_stacking_threads() {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let held = sem.clone().try_acquire_owned().unwrap();
        let out = bounded_with(sem, || 42).await;
        match out {
            Err((code, msg)) => {
                assert_eq!(code, StatusCode::TOO_MANY_REQUESTS);
                assert!(msg.contains("saturated"));
            }
            Ok(_) => panic!("must refuse when no evaluation threads are free"),
        }
        drop(held);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn regex_spans_are_utf16_units_in_both_modes() {
        let subject = "héllo world".to_string();
        let plain = eval_regex(Json(EvalRegexRequest {
            pattern: "world".to_string(),
            flags: String::new(),
            subject: subject.clone(),
            mode: "matches".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert_eq!(plain.spans, vec![(6, 11)]);

        let jq = eval_regex(Json(EvalRegexRequest {
            pattern: "world".to_string(),
            flags: String::new(),
            subject,
            mode: "jq".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert_eq!(jq.spans, plain.spans, "both engines report the same unit");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn regex_spans_cover_astral_characters() {
        let out = eval_regex(Json(EvalRegexRequest {
            pattern: "end".to_string(),
            flags: String::new(),
            subject: "🚀 end".to_string(),
            mode: "matches".to_string(),
        }))
        .await
        .unwrap()
        .0;
        assert_eq!(out.spans, vec![(3, 6)]);
    }

    #[tokio::test]
    async fn a_request_of_more_than_two_hundred_expressions_is_refused() {
        let mut many = req(json!({"ok": true}), ".ok");
        many.expressions = vec![".ok".to_string(); MAX_EXPRESSIONS + 1];
        let (status, said) = eval_assert(Json(many)).await.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(said.contains("201"), "{said}");

        let mut exactly = req(json!({"ok": true}), ".ok");
        exactly.expressions = vec![".ok == true".to_string(); MAX_EXPRESSIONS];
        let verdicts = eval_assert(Json(exactly)).await.unwrap().0;
        assert_eq!(verdicts.len(), MAX_EXPRESSIONS);
    }
}
