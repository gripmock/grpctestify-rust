use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

pub struct UuidPlugin;

impl Plugin for UuidPlugin {
    fn name(&self) -> &str {
        "uuid"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid UUID string"
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
                "uuid: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if uuid::Uuid::parse_str(s).is_ok() {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid UUID, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for UUID check, got {:?}",
                arg
            )))),
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
    fn test_uuid_plugin_name() {
        let plugin = UuidPlugin;
        assert_eq!(plugin.name(), "uuid");
    }

    #[test]
    fn test_uuid_plugin_description() {
        let plugin = UuidPlugin;
        assert!(plugin.description().contains("UUID"));
    }

    #[test]
    fn test_uuid_plugin_valid_uuid() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::String(
                "550e8400-e29b-41d4-a716-446655440000".to_string(),
            )],
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
    fn test_uuid_plugin_invalid_uuid() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-uuid".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_uuid_plugin_wrong_type() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_uuid_plugin_no_args() {
        let plugin = UuidPlugin;
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
    fn test_uuid_plugin_too_many_args() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("test".to_string()),
                Value::String("test2".to_string()),
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
}
