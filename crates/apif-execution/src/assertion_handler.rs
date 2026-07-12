use apif_assert::{AssertionEngine, AssertionResult};
use apif_plugins::core::PluginManager;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

static PLUGIN_REGISTRY: LazyLock<Arc<dyn apif_assert::registry::PluginRegistry>> =
    LazyLock::new(|| Arc::new(PluginManager::new()));

#[derive(Default)]
pub struct AssertionHandler {
    engine: AssertionEngine,
}

impl AssertionHandler {
    pub fn new() -> Self {
        Self {
            engine: AssertionEngine::with_registry(PLUGIN_REGISTRY.clone()),
        }
    }

    pub fn run_assertions(
        &self,
        _assertions: &[String],
        _response: &Value,
        _headers: Option<&HashMap<String, String>>,
        _trailers: Option<&HashMap<String, String>>,
    ) -> Vec<String> {
        let mut failures = Vec::new();
        for expr in _assertions {
            match self.engine.evaluate(expr, _response, _headers, _trailers) {
                Ok(AssertionResult::Pass) => {}
                Ok(AssertionResult::Fail { message, .. }) => {
                    failures.push(format!("{}: {}", expr, message))
                }
                Ok(AssertionResult::Error(e)) => failures.push(format!("{}: {}", expr, e)),
                Err(e) => failures.push(format!("{}: {}", expr, e)),
            }
        }
        failures
    }

    pub fn evaluate_single_assertion(
        &self,
        expression: &str,
        response: &Value,
        headers: Option<&HashMap<String, String>>,
        trailers: Option<&HashMap<String, String>>,
    ) -> AssertionResult {
        self.engine
            .evaluate(expression, response, headers, trailers)
            .unwrap_or(AssertionResult::Error("evaluation error".into()))
    }

    pub fn append_single_failure(
        failures: &mut Vec<String>,
        result: &AssertionResult,
        expression: &str,
    ) {
        match result {
            AssertionResult::Fail { message, .. } => {
                failures.push(format!("{}: {}", expression, message))
            }
            AssertionResult::Error(msg) => failures.push(format!("{}: {}", expression, msg)),
            _ => {}
        }
    }
}
