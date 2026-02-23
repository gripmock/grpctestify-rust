use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct UrlPlugin;

impl Plugin for UrlPlugin {
    fn name(&self) -> &str {
        "url"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid URL"
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "url: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if url::Url::parse(s).is_ok() {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid URL, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for URL check, got {:?}",
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
    fn test_url_plugin_name() {
        let plugin = UrlPlugin;
        assert_eq!(plugin.name(), "url");
    }

    #[test]
    fn test_url_plugin_valid_url() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("https://example.com".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_url_plugin_invalid_url() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-url".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_url_plugin_no_args() {
        let plugin = UrlPlugin;
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
