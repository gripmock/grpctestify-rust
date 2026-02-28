use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

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
            return_kind: PluginReturnKind::Boolean,
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
        PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        }
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
}
