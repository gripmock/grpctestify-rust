use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct LenPlugin;

impl Plugin for LenPlugin {
    fn name(&self) -> &str {
        "len"
    }

    fn description(&self) -> &str {
        "Returns the length of a string or array"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "len: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg {
            Value::String(s) => Ok(PluginResult::Value(Value::Number(
                serde_json::Number::from(s.len()),
            ))),
            Value::Array(arr) => Ok(PluginResult::Value(Value::Number(
                serde_json::Number::from(arr.len()),
            ))),
            _ => Ok(PluginResult::Value(Value::Null)),
        }
    }
}
