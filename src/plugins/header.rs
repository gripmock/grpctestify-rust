use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct HeaderPlugin;

impl Plugin for HeaderPlugin {
    fn name(&self) -> &str {
        "header"
    }

    fn description(&self) -> &str {
        "Checks if a specific header exists"
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "header: expects exactly 1 argument (header name)".to_string(),
            )));
        }

        let key = match args[0].as_str() {
            Some(s) => s.to_lowercase(),
            None => {
                return Ok(PluginResult::Assertion(AssertionResult::Error(
                    "header: argument must be a string".to_string(),
                )));
            }
        };

        if let Some(headers) = context.headers {
            // Check if header exists (case-insensitive lookup, assuming headers map keys are already lowercase or we iterate)
            // In the original engine, keys seemed to be lowercased.
            // Let's assume standard behavior: headers are case-insensitive.
            // The map keys should probably be normalized.
            // Based on engine.rs: if m.contains_key(&key)

            if headers.contains_key(&key) {
                Ok(PluginResult::Assertion(AssertionResult::Pass))
            } else {
                Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                    "Header '{}' not found",
                    key
                ))))
            }
        } else {
            Ok(PluginResult::Assertion(AssertionResult::Error(
                "Headers not available in this context".to_string(),
            )))
        }
    }
}
