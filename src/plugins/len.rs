use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

pub struct LenPlugin;

impl Plugin for LenPlugin {
    fn name(&self) -> &str {
        "len"
    }

    fn description(&self) -> &str {
        "Returns the length of a string or array"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Number,
            purity: PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["value"],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        }
    }

    #[test]
    fn test_len_plugin_name() {
        let plugin = LenPlugin;
        assert_eq!(plugin.name(), "len");
    }

    #[test]
    fn test_len_plugin_string_length() {
        let plugin = LenPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("hello".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Number(n)) = result.unwrap() {
            assert_eq!(n.as_u64().unwrap(), 5);
        } else {
            panic!("Expected Value result with number");
        }
    }

    #[test]
    fn test_len_plugin_array_length() {
        let plugin = LenPlugin;
        let context = create_context();
        let arr = Value::Array(vec![
            Value::Number(serde_json::Number::from(1)),
            Value::Number(serde_json::Number::from(2)),
            Value::Number(serde_json::Number::from(3)),
        ]);
        let result = plugin.execute(&[arr], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Number(n)) = result.unwrap() {
            assert_eq!(n.as_u64().unwrap(), 3);
        } else {
            panic!("Expected Value result with number");
        }
    }

    #[test]
    fn test_len_plugin_empty_string() {
        let plugin = LenPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Number(n)) = result.unwrap() {
            assert_eq!(n.as_u64().unwrap(), 0);
        } else {
            panic!("Expected Value result with number");
        }
    }

    #[test]
    fn test_len_plugin_null_type() {
        let plugin = LenPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Null], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Null) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Null result");
        }
    }

    #[test]
    fn test_len_plugin_no_args() {
        let plugin = LenPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }
}
