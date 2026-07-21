// Environment variable plugin
// Reads environment variables: @env("VAR_NAME") or @env("VAR_NAME", "default_value")
// Returns: the value of the variable, or the default value if not set

use anyhow::Result;
use serde_json::Value;
use std::env;
use std::env::VarError;

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
            replacement: None,
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
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

        let var_name = match &args[0] {
            Value::String(name) => name,
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@env first argument must be a string (variable name)",
                )));
            }
        };

        let default_value = if args.len() > 1 {
            match &args[1] {
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            }
        } else {
            None
        };

        // SECURITY: `@env` exposes the process environment to test files. Any
        // variable readable by the process (including secrets such as tokens or
        // credentials) can be pulled into a test via `@env("SECRET")`. There is
        // currently no allowlist mechanism in the plugin context to scope this,
        // so operators should treat `.apif`/`.gctf` files as trusted input and
        // avoid running them in environments holding sensitive variables.
        //
        // Get environment variable. We deliberately distinguish the three cases:
        //   * present + valid UTF-8 -> return the value
        //   * present + non-UTF-8   -> hard error (do NOT silently fall back to
        //     the default or Null, which would mask a real misconfiguration)
        //   * not present           -> default value, else Null
        match env::var(var_name) {
            Ok(value) => Ok(PluginResult::Value(Value::String(value))),
            Err(VarError::NotUnicode(_)) => Ok(PluginResult::Assertion(AssertionResult::fail(
                format!("@env: variable '{var_name}' is set but its value is not valid UTF-8"),
            ))),
            Err(VarError::NotPresent) => {
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

    #[cfg(unix)]
    #[test]
    fn test_env_plugin_non_utf8_errors_not_treated_as_unset() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // Arrange: set a variable to a value that is not valid UTF-8.
        let plugin = EnvPlugin;
        unsafe {
            env::set_var("TEST_VAR_NON_UTF8", OsStr::from_bytes(&[0x66, 0x80, 0x6f]));
        }

        // Act: a default is supplied, but a present-but-non-UTF8 var must NOT
        // silently fall back to it — it is a distinct error condition.
        let result = plugin
            .execute(
                &[
                    Value::String("TEST_VAR_NON_UTF8".to_string()),
                    Value::String("fallback".to_string()),
                ],
                &PluginContext::new(&Value::Null),
            )
            .unwrap();

        // Assert
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));

        unsafe {
            env::remove_var("TEST_VAR_NON_UTF8");
        }
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
