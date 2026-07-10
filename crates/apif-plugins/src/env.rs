// Environment variable plugin
// Reads environment variables: @env("VAR_NAME") or @env("VAR_NAME", "default_value")
// Returns: the value of the variable, or the default value if not set

use anyhow::Result;
use serde_json::Value;
use std::env;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

/// Environment variable plugin
#[derive(Debug, Clone, Default)]
pub struct EnvPlugin;

impl Plugin for EnvPlugin {
    fn name(&self) -> &'static str {
        "env"
    }

    fn description(&self) -> &'static str {
        "Read environment variable value. Optional second argument provides a default value."
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            arg_types: &[
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: true,
                    default: None,
                },
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: false,
                    default: None,
                },
            ],
            purity: PluginPurity::Impure,
            deterministic: false,
            idempotent: false,
            safe_for_rewrite: false,
            arg_names: &["name", "default"],
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        // Check argument count
        if args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@env requires at least 1 argument: the variable name",
            )));
        }

        if args.len() > 2 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@env accepts at most 2 arguments: name and optional default value",
            )));
        }

        // Get variable name
        let var_name = match &args[0] {
            Value::String(name) => name,
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@env first argument must be a string (variable name)",
                )));
            }
        };

        // Get optional default value
        let default_value = if args.len() > 1 {
            match &args[1] {
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            }
        } else {
            None
        };

        // Get environment variable
        match env::var(var_name) {
            Ok(value) => Ok(PluginResult::Value(Value::String(value))),
            Err(_) => {
                if let Some(default) = default_value {
                    Ok(PluginResult::Value(Value::String(default)))
                } else {
                    Ok(PluginResult::Value(Value::Null))
                }
            }
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
                &PluginContext::new(&Value::Null),
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
                &PluginContext::new(&Value::Null),
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
            .execute(&[], &PluginContext::new(&Value::Null))
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

        // Act — 3 args is too many (max 2)
        let result = plugin
            .execute(
                &[
                    Value::String("VAR1".to_string()),
                    Value::String("default".to_string()),
                    Value::String("extra".to_string()),
                ],
                &PluginContext::new(&Value::Null),
            )
            .unwrap();

        // Assert
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_env_plugin_with_default_value() {
        // Arrange
        let plugin = EnvPlugin;

        // Act — var doesn't exist, should return default
        let result = plugin
            .execute(
                &[
                    Value::String("NONEXISTENT_VAR_12345".to_string()),
                    Value::String("/home/user".to_string()),
                ],
                &PluginContext::new(&Value::Null),
            )
            .unwrap();

        // Assert
        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "/home/user"));
    }

    #[test]
    fn test_env_plugin_var_exists_ignores_default() {
        // Arrange
        let plugin = EnvPlugin;
        unsafe {
            env::set_var("TEST_VAR_DEFAULT", "actual_value");
        }

        // Act — var exists, should return actual value not default
        let result = plugin
            .execute(
                &[
                    Value::String("TEST_VAR_DEFAULT".to_string()),
                    Value::String("ignored_default".to_string()),
                ],
                &PluginContext::new(&Value::Null),
            )
            .unwrap();

        // Assert
        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "actual_value"));

        unsafe {
            env::remove_var("TEST_VAR_DEFAULT");
        }
    }

    #[test]
    fn test_env_plugin_wrong_type() {
        // Arrange
        let plugin = EnvPlugin;

        // Act
        let result = plugin
            .execute(
                &[Value::Number(123.into())],
                &PluginContext::new(&Value::Null),
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
        assert_eq!(plugin.name(), "env");
    }

    #[test]
    fn test_env_plugin_description() {
        let plugin = EnvPlugin;
        assert!(plugin.description().contains("environment"));
    }
}
