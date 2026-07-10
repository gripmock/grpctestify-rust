use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

pub struct EmptyPlugin;

impl Plugin for EmptyPlugin {
    fn name(&self) -> &str {
        "empty"
    }

    fn description(&self) -> &str {
        "Checks whether a value is empty (null, empty string, empty array, empty object)"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Bool,
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
                "empty: expects exactly 1 argument".to_string(),
            )));
        }

        let is_empty = match &args[0] {
            Value::Null => true,
            Value::String(s) => s.is_empty(),
            Value::Array(values) => values.is_empty(),
            Value::Object(map) => map.is_empty(),
            _ => false,
        };

        Ok(PluginResult::Value(Value::Bool(is_empty)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn test_empty_plugin_name() {
        let plugin = EmptyPlugin;
        assert_eq!(plugin.name(), "empty");
    }

    #[test]
    fn test_empty_plugin_empty_string() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin
            .execute(&[Value::String(String::new())], &context)
            .unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_empty_plugin_non_empty_string() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin
            .execute(&[Value::String("hello".to_string())], &context)
            .unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_empty_plugin_empty_array() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Array(vec![])], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_empty_plugin_null() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Null], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_empty_plugin_empty_object() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin
            .execute(&[Value::Object(serde_json::Map::new())], &context)
            .unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_empty_plugin_non_empty_object() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let obj = serde_json::json!({"key": "value"});
        let result = plugin.execute(&[obj], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_empty_plugin_non_empty_array() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let arr = Value::Array(vec![Value::Number(serde_json::Number::from(1))]);
        let result = plugin.execute(&[arr], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_empty_plugin_number_type() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin
            .execute(&[Value::Number(serde_json::Number::from(42))], &context)
            .unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_empty_plugin_bool_type() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Bool(true)], &context).unwrap();
        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_empty_plugin_no_args() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context).unwrap();
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_empty_plugin_too_many_args() {
        let plugin = EmptyPlugin;
        let context = create_context();
        let result = plugin
            .execute(
                &[
                    Value::String("a".to_string()),
                    Value::String("b".to_string()),
                ],
                &context,
            )
            .unwrap();
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_empty_plugin_description() {
        let plugin = EmptyPlugin;
        assert!(plugin.description().contains("empty"));
    }

    #[test]
    fn test_empty_plugin_signature() {
        let plugin = EmptyPlugin;
        let sig = plugin.signature();
        assert_eq!(sig.arg_names, &["value"]);
        assert!(sig.safe_for_rewrite);
        assert!(sig.idempotent);
        assert!(sig.deterministic);
    }
}
