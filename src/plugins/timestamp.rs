use anyhow::Result;
use chrono::DateTime;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct TimestampPlugin;

impl Plugin for TimestampPlugin {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid RFC3339 timestamp"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "timestamp: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if DateTime::parse_from_rfc3339(s).is_ok() {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid RFC3339 timestamp, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for timestamp check, got {:?}",
                arg
            )))),
        }
    }
}
