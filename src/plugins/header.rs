use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct HeaderPlugin;

impl Plugin for HeaderPlugin {
    fn name(&self) -> &str {
        "header"
    }

    fn description(&self) -> &str {
        "Checks if a specific header exists"
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "header: expects exactly 1 argument (header name)".to_string(),
            )));
        }

        let key = match args[0].as_str() {
            Some(s) => s.to_lowercase(),
            None => {
                return Ok(PluginResult::Assertion(AssertionResult::Error(
                    "header: argument must be a string".to_string(),
                )));
            }
        };

        if let Some(headers) = context.headers {
            if headers.contains_key(&key) {
                Ok(PluginResult::Assertion(AssertionResult::Pass))
            } else {
                Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                    "Header '{}' not found",
                    key
                ))))
            }
        } else {
            Ok(PluginResult::Assertion(AssertionResult::Error(
                "Headers not available in this context".to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_header_plugin_name() {
        let plugin = HeaderPlugin;
        assert_eq!(plugin.name(), "header");
    }

    #[test]
    fn test_header_plugin_exists() {
        let plugin = HeaderPlugin;
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let context = PluginContext {
            response: &Value::Null,
            headers: Some(&headers),
            trailers: None,
        };
        let result = plugin.execute(&[Value::String("content-type".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_header_plugin_not_exists() {
        let plugin = HeaderPlugin;
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let context = PluginContext {
            response: &Value::Null,
            headers: Some(&headers),
            trailers: None,
        };
        let result = plugin.execute(&[Value::String("x-custom".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_header_plugin_no_headers() {
        let plugin = HeaderPlugin;
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        };
        let result = plugin.execute(&[Value::String("content-type".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("Headers not available"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_header_plugin_no_args() {
        let plugin = HeaderPlugin;
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        };
        let result = plugin.execute(&[], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_header_plugin_wrong_arg_type() {
        let plugin = HeaderPlugin;
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        };
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("must be a string"));
        } else {
            panic!("Expected Error assertion result");
        }
    }
}
