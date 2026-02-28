// Environment variable plugin for EXTRACT section
// Reads environment variables: @env("VAR_NAME")
// Returns: {"value": "...", "exists": true/false}

use anyhow::Result;
use serde_json::Value;
use std::env;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

/// Environment variable plugin
#[derive(Debug, Clone, Default)]
pub struct EnvPlugin;

impl Plugin for EnvPlugin {
    fn name(&self) -> &'static str {
        "@env"
    }

    fn description(&self) -> &'static str {
        "Read environment variable value"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::String,
            purity: PluginPurity::ContextDependent,
            deterministic: false,
            idempotent: false,
            safe_for_rewrite: false,
            arg_names: &["name"],
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        // Check argument count
        if args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@env requires 1 argument: the variable name",
            )));
        }

        if args.len() > 1 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@env accepts only 1 argument",
            )));
        }

        // Get variable name
        let var_name = match &args[0] {
            Value::String(name) => name,
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@env argument must be a string",
                )));
            }
        };

        // Get environment variable
        match env::var(var_name) {
            Ok(value) => Ok(PluginResult::Value(Value::String(value))),
            Err(_) => Ok(PluginResult::Value(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_plugin_exists() {
        // Arrange
        let plugin = EnvPlugin;
        unsafe {
            env::set_var("TEST_VAR", "test_value");
        }

        // Act
        let result = plugin
            .execute(
                &[Value::String("TEST_VAR".to_string())],
                &PluginContext {
                    response: &Value::Null,
                    headers: None,
                    trailers: None,
                },
            )
            .unwrap();

        // Assert
        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "test_value"));

        unsafe {
            env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_env_plugin_not_exists() {
        // Arrange
        let plugin = EnvPlugin;

        // Act
        let result = plugin
            .execute(
                &[Value::String("NONEXISTENT_VAR_12345".to_string())],
                &PluginContext {
                    response: &Value::Null,
                    headers: None,
                    trailers: None,
                },
            )
            .unwrap();

        // Assert
        assert!(matches!(result, PluginResult::Value(Value::Null)));
    }

    #[test]
    fn test_env_plugin_no_args() {
        // Arrange
        let plugin = EnvPlugin;

        // Act
        let result = plugin
            .execute(
                &[],
                &PluginContext {
                    response: &Value::Null,
                    headers: None,
                    trailers: None,
                },
            )
            .unwrap();

        // Assert
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_env_plugin_too_many_args() {
        // Arrange
        let plugin = EnvPlugin;

        // Act
        let result = plugin
            .execute(
                &[
                    Value::String("VAR1".to_string()),
                    Value::String("VAR2".to_string()),
                ],
                &PluginContext {
                    response: &Value::Null,
                    headers: None,
                    trailers: None,
                },
            )
            .unwrap();

        // Assert
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_env_plugin_wrong_type() {
        // Arrange
        let plugin = EnvPlugin;

        // Act
        let result = plugin
            .execute(
                &[Value::Number(123.into())],
                &PluginContext {
                    response: &Value::Null,
                    headers: None,
                    trailers: None,
                },
            )
            .unwrap();

        // Assert
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_env_plugin_name() {
        let plugin = EnvPlugin;
        assert_eq!(plugin.name(), "@env");
    }

    #[test]
    fn test_env_plugin_description() {
        let plugin = EnvPlugin;
        assert!(plugin.description().contains("environment"));
    }
}
