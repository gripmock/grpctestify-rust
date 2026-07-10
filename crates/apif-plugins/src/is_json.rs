use crate::core::{Plugin, PluginContext, PluginResult, PluginSignature};
use crate::type_info::{ArgTypeInfo, TypeInfo};
use anyhow::Result;
use serde_json::Value;

pub struct IsJsonPlugin;

impl Plugin for IsJsonPlugin {
    fn name(&self) -> &str {
        "is_json"
    }

    fn description(&self) -> &str {
        "Checks whether a string value is valid JSON"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Bool(false)));
        }
        let val = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Bool(false))),
        };
        let is_valid = serde_json::from_str::<Value>(val).is_ok();
        Ok(PluginResult::Value(Value::Bool(is_valid)))
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Bool,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
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
    use serde_json::json;

    fn ctx() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn test_is_json_name() {
        assert_eq!(IsJsonPlugin.name(), "is_json");
    }

    #[test]
    fn test_is_json_description() {
        assert!(!IsJsonPlugin.description().is_empty());
    }

    #[test]
    fn test_is_json_signature() {
        let sig = IsJsonPlugin.signature();
        assert_eq!(sig.return_type, TypeInfo::Bool);
        assert!(sig.safe_for_rewrite);
    }

    #[test]
    fn test_is_json_valid_object() {
        assert_eq!(
            IsJsonPlugin
                .execute(&[json!(r#"{"key":"value"}"#)], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_is_json_valid_array() {
        assert_eq!(
            IsJsonPlugin.execute(&[json!("[1,2,3]")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_is_json_invalid() {
        assert_eq!(
            IsJsonPlugin.execute(&[json!("not json")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_json_non_string() {
        assert_eq!(
            IsJsonPlugin.execute(&[json!(42)], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_json_no_args() {
        assert_eq!(
            IsJsonPlugin.execute(&[], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }
}
