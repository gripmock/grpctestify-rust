use anyhow::Result;
use chrono::DateTime;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

pub struct TimestampPlugin;

impl Plugin for TimestampPlugin {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid RFC3339 timestamp"
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
                "timestamp: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if DateTime::parse_from_rfc3339(s).is_ok() {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid RFC3339 timestamp, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for timestamp check, got {:?}",
                arg
            )))),
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
    fn test_timestamp_plugin_name() {
        let plugin = TimestampPlugin;
        assert_eq!(plugin.name(), "timestamp");
    }

    #[test]
    fn test_timestamp_plugin_valid() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::String("2024-01-15T10:30:00Z".to_string())],
            &context,
        );
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_timestamp_plugin_invalid() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-timestamp".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_timestamp_plugin_no_args() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_timestamp_plugin_too_many_args() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("2024-01-15T10:30:00Z".to_string()),
                Value::String("extra".to_string()),
            ],
            &context,
        );
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_timestamp_plugin_wrong_type() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::Number(serde_json::Number::from(1234567890))],
            &context,
        );
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_timestamp_plugin_description() {
        let plugin = TimestampPlugin;
        assert!(plugin.description().contains("RFC3339"));
    }

    #[test]
    fn test_timestamp_plugin_signature() {
        let plugin = TimestampPlugin;
        let sig = plugin.signature();
        assert_eq!(sig.arg_names, &["value"]);
        assert!(sig.safe_for_rewrite);
    }
}
