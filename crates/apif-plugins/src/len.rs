use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

pub struct LenPlugin;

impl Plugin for LenPlugin {
    fn name(&self) -> &str {
        "len"
    }

    fn description(&self) -> &str {
        "Returns the length of a string or array as a non-negative integer"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::UInt,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::Any,
                required: true,
                default: None,
            }],
            purity: PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["value"],
            replacement: None,
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
            // Match jq `length`: count Unicode scalar values (codepoints), not bytes.
            Value::String(s) => Ok(PluginResult::Value(Value::Number(
                serde_json::Number::from(s.chars().count()),
            ))),
            Value::Array(arr) => Ok(PluginResult::Value(Value::Number(
                serde_json::Number::from(arr.len()),
            ))),
            // jq `length` on an object counts its entries.
            Value::Object(map) => Ok(PluginResult::Value(Value::Number(
                serde_json::Number::from(map.len()),
            ))),
            _ => Ok(PluginResult::Value(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
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
    fn test_len_plugin_unicode_string_counts_codepoints() {
        // Regression: "привет" is 12 bytes but 6 codepoints; jq `length` == 6.
        let plugin = LenPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("привет".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Number(n)) = result.unwrap() {
            assert_eq!(n.as_u64().unwrap(), 6);
        } else {
            panic!("Expected Value result with number");
        }
    }

    #[test]
    fn test_len_plugin_object_counts_entries() {
        let plugin = LenPlugin;
        let context = create_context();
        let obj = serde_json::json!({"a": 1, "b": 2});
        let result = plugin.execute(&[obj], &context);
        assert!(result.is_ok());
        if let PluginResult::Value(Value::Number(n)) = result.unwrap() {
            assert_eq!(n.as_u64().unwrap(), 2);
        } else {
            panic!("Expected Value result with number");
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
