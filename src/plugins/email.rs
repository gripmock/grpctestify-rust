use anyhow::Result;
use email_address::EmailAddress;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct EmailPlugin;

impl Plugin for EmailPlugin {
    fn name(&self) -> &str {
        "email"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid email address"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "email: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if EmailAddress::is_valid(s) {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid email, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for email check, got {:?}",
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
    fn test_email_plugin_name() {
        let plugin = EmailPlugin;
        assert_eq!(plugin.name(), "email");
    }

    #[test]
    fn test_email_plugin_valid_email() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("test@example.com".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_email_plugin_invalid_email() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-an-email".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_email_plugin_wrong_type() {
        let plugin = EmailPlugin;
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
    fn test_email_plugin_no_args() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }
}
