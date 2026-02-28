// Regex plugin for pattern matching
// Usage: @regex(field, "pattern")

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

/// Regex plugin for pattern matching
#[derive(Debug, Clone, Default)]
pub struct RegexPlugin;

impl Plugin for RegexPlugin {
    fn name(&self) -> &'static str {
        "@regex"
    }

    fn description(&self) -> &'static str {
        "Validate field matches regex pattern"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Boolean,
            purity: PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["value", "pattern"],
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        // Check argument count
        if args.len() != 2 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@regex requires 2 arguments: field and pattern",
            )));
        }

        // Get field value
        let field_value = match &args[0] {
            Value::String(s) => s,
            Value::Number(n) => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                    "@regex expects string field, got number: {}",
                    n
                ))));
            }
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@regex first argument must be a string",
                )));
            }
        };

        // Get pattern
        let pattern = match &args[1] {
            Value::String(s) => s,
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@regex second argument (pattern) must be a string",
                )));
            }
        };

        // Compile and match regex
        match Regex::new(pattern) {
            Ok(re) => {
                if re.is_match(field_value) {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Value '{}' does not match pattern '{}'",
                        field_value, pattern
                    ))))
                }
            }
            Err(e) => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Invalid regex pattern '{}': {}",
                pattern, e
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
    fn test_regex_plugin_valid_email() {
        let plugin = RegexPlugin;
        let result = plugin
            .execute(
                &[
                    Value::String("test@example.com".to_string()),
                    Value::String(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ],
                &create_context(),
            )
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Pass)
        ));
    }

    #[test]
    fn test_regex_plugin_invalid_email() {
        let plugin = RegexPlugin;
        let result = plugin
            .execute(
                &[
                    Value::String("invalid-email".to_string()),
                    Value::String(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ],
                &create_context(),
            )
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_regex_plugin_uuid() {
        let plugin = RegexPlugin;
        let result = plugin
            .execute(
                &[
                    Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
                    Value::String(
                        r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
                            .to_string(),
                    ),
                ],
                &create_context(),
            )
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Pass)
        ));
    }

    #[test]
    fn test_regex_plugin_invalid_pattern() {
        let plugin = RegexPlugin;
        let result = plugin
            .execute(
                &[
                    Value::String("test".to_string()),
                    Value::String(r"[invalid(regex".to_string()),
                ],
                &create_context(),
            )
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_regex_plugin_wrong_arg_count() {
        let plugin = RegexPlugin;
        let result = plugin
            .execute(&[Value::String("test".to_string())], &create_context())
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_regex_plugin_name() {
        let plugin = RegexPlugin;
        assert_eq!(plugin.name(), "@regex");
    }

    #[test]
    fn test_regex_plugin_description() {
        let plugin = RegexPlugin;
        assert!(plugin.description().contains("regex"));
    }
}
