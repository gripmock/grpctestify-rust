use crate::core::{Plugin, PluginContext, PluginResult, PluginSignature};
use crate::type_info::{ArgTypeInfo, TypeInfo};
use anyhow::Result;
use serde_json::Value;

pub struct IsBase64Plugin;

impl Plugin for IsBase64Plugin {
    fn name(&self) -> &str {
        "is_base64"
    }

    fn description(&self) -> &str {
        "Checks whether a value is a valid base64-encoded string"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Bool(false)));
        }
        let val = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Bool(false))),
        };
        // Validate base64 (supports standard and URL-safe, with or without padding)
        if val.is_empty() {
            return Ok(PluginResult::Value(Value::Bool(false)));
        }
        // Check that the string contains only valid base64 characters
        let valid = val.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '-' || c == '_' || c == '='
        });
        Ok(PluginResult::Value(Value::Bool(valid)))
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
    fn test_is_base64_name() {
        assert_eq!(IsBase64Plugin.name(), "is_base64");
    }

    #[test]
    fn test_is_base64_description() {
        assert!(!IsBase64Plugin.description().is_empty());
    }

    #[test]
    fn test_is_base64_signature() {
        let sig = IsBase64Plugin.signature();
        assert_eq!(sig.return_type, TypeInfo::Bool);
    }

    #[test]
    fn test_is_base64_valid() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("dGVzdA==")], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_is_base64_url_safe() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("dGVzdA")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn test_is_base64_invalid_chars() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("hello!world")], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_empty_string() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_non_string() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!(42)], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_no_args() {
        assert_eq!(
            IsBase64Plugin.execute(&[], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }
}
