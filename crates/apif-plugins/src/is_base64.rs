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

/// Validate a base64-encoded string.
///
/// Accepts both the standard (`+`, `/`) and URL-safe (`-`, `_`) alphabets, with
/// or without trailing `=` padding. Enforces correct length and padding so that
/// malformed inputs such as `"A"` (a lone char cannot encode any byte) or
/// `"===="` (padding with no data) are rejected.
fn is_valid_base64(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // `=` is a single-byte ASCII char, so byte-length arithmetic is safe here.
    let data = s.trim_end_matches('=');
    let padding = s.len() - data.len();
    if padding > 2 || data.is_empty() {
        return false;
    }

    // Data portion must contain only alphabet characters (no interior padding).
    let alphabet_ok = data
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '-' || c == '_');
    if !alphabet_ok {
        return false;
    }

    // After the alphabet check, `data` is guaranteed ASCII so byte len == char count.
    let rem = data.len() % 4;
    if padding > 0 {
        // With padding the total length must be a multiple of 4, and the amount
        // of padding must match the number of encoded bytes in the final group.
        if !s.len().is_multiple_of(4) {
            return false;
        }
        match rem {
            2 => padding == 2,
            3 => padding == 1,
            _ => false,
        }
    } else {
        // Without padding a remainder of 1 is impossible (a single char can't
        // encode a whole byte); 0, 2 and 3 are valid.
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
    fn test_is_base64_rejects_single_char() {
        // Regression: a lone char cannot encode any byte; must be rejected.
        assert_eq!(
            IsBase64Plugin.execute(&[json!("A")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_rejects_padding_only() {
        // Regression: padding with no data must be rejected.
        assert_eq!(
            IsBase64Plugin.execute(&[json!("====")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_rejects_bad_length_with_padding() {
        // Length not a multiple of 4 while padded is invalid.
        assert_eq!(
            IsBase64Plugin.execute(&[json!("dGVzdA=")], &ctx()).unwrap(),
            PluginResult::Value(Value::Bool(false))
        );
    }

    #[test]
    fn test_is_base64_rejects_interior_padding() {
        assert_eq!(
            IsBase64Plugin
                .execute(&[json!("dG=VzdA==")], &ctx())
                .unwrap(),
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
