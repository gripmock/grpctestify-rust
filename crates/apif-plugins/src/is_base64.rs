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
        Ok(PluginResult::Value(Value::Bool(is_valid_base64(val))))
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

fn is_valid_base64(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let data = s.trim_end_matches('=');
    let padding = s.len() - data.len();
    if padding > 2 || data.is_empty() {
        return false;
    }

    let alphabet_ok = data
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '-' || c == '_');
    if !alphabet_ok {
        return false;
    }

    let rem = data.len() % 4;
    if padding > 0 {
        if !s.len().is_multiple_of(4) {
            return false;
        }
        match rem {
            2 => padding == 2,
            3 => padding == 1,
            _ => false,
        }
    } else {
        rem != 1
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
    fn is_base64_name() {
        assert_eq!(IsBase64Plugin.name(), "is_base64");
    }

    #[test]
    fn is_base64_description() {
        assert!(!IsBase64Plugin.description().is_empty());
    }

    #[test]
    fn is_base64_signature() {
        let sig = IsBase64Plugin.signature();
        assert_eq!(sig.return_type, TypeInfo::Bool);
    }

    #[test]
    fn is_base64_valid() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("dGVzdA==")], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn is_base64_url_safe() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("dGVzdA")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(true))
        );
    }

    #[test]
    fn is_base64_invalid_chars() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("hello!world")], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_empty_string() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_non_string() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!(42)], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_rejects_single_char() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("A")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_rejects_padding_only() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("====")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_rejects_bad_length_with_padding() {
        assert_eq!(
            IsBase64Plugin.execute(&[json!("dGVzdA=")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_rejects_interior_padding() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("dG=VzdA==")], &ctx())
                .unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn is_base64_no_args() {
        assert_eq!(
            IsBase64Plugin.execute(&[], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }
}
