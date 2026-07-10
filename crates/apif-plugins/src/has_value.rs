use crate::core::{Plugin, PluginContext, PluginResult, PluginSignature};
use crate::type_info::{ArgTypeInfo, TypeInfo};
use anyhow::Result;
use serde_json::Value;

pub struct HasValuePlugin;

impl Plugin for HasValuePlugin {
    fn name(&self) -> &str {
        "has_value"
    }

    fn description(&self) -> &str {
        "Checks whether a value is non-empty (has value)"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Bool(false)));
        }
        let val = &args[0];
        let has_value = match val {
            Value::Null => false,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
            Value::Bool(_) | Value::Number(_) => true,
        };
        Ok(PluginResult::Value(Value::Bool(has_value)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Bool,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::Any,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["value"],
            replacement: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn test_has_value_name() {
        assert_eq!(HasValuePlugin.name(), "has_value");
    }

    #[test]
    fn test_has_value_description() {
        assert!(!HasValuePlugin.description().is_empty());
    }

    #[test]
    fn test_has_value_signature() {
        let sig = HasValuePlugin.signature();
        assert_eq!(sig.return_type, TypeInfo::Bool);
        assert!(sig.safe_for_rewrite);
        assert!(sig.deterministic);
    }

    #[test]
    fn test_has_value_null() {
        assert_eq!(
            HasValuePlugin.execute(&[Value::Null], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_has_value_empty_string() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::String("".into())], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_has_value_non_empty_string() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::String("hello".into())], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_has_value_empty_array() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::Array(vec![])], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_has_value_non_empty_array() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::Array(vec![Value::Bool(true)])], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_has_value_empty_object() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::Object(serde_json::Map::new())], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_has_value_non_empty_object() {
        assert_eq!(
            HasValuePlugin
                .execute(&[serde_json::json!({"k": "v"})], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_has_value_bool() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::Bool(true)], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_has_value_number() {
        assert_eq!(
            HasValuePlugin
                .execute(&[Value::Number(0.into())], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_has_value_no_args() {
        assert_eq!(
            HasValuePlugin.execute(&[], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }
}
