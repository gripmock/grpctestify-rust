use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct TrailerPlugin;

impl Plugin for TrailerPlugin {
    fn name(&self) -> &str {
        "trailer"
    }

    fn description(&self) -> &str {
        "Checks if a specific trailer exists"
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "trailer: expects exactly 1 argument (trailer name)".to_string(),
            )));
        }

        let key = match args[0].as_str() {
            Some(s) => s.to_lowercase(),
            None => {
                return Ok(PluginResult::Assertion(AssertionResult::Error(
                    "trailer: argument must be a string".to_string(),
                )));
            }
        };

        if let Some(trailers) = context.trailers {
            if trailers.contains_key(&key) {
                Ok(PluginResult::Assertion(AssertionResult::Pass))
            } else {
                Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                    "Trailer '{}' not found",
                    key
                ))))
            }
        } else {
            Ok(PluginResult::Assertion(AssertionResult::Error(
                "Trailers not available in this context".to_string(),
            )))
        }
    }
}
