use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{Plugin, PluginContext, PluginResult};

pub struct TrailerPlugin;

impl Plugin for TrailerPlugin {
    fn name(&self) -> &str {
        "trailer"
    }

    fn description(&self) -> &str {
        "Checks if a specific trailer exists"
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "trailer: expects exactly 1 argument (trailer name)".to_string(),
            )));
        }

        let key = match args[0].as_str() {
            Some(s) => s.to_lowercase(),
            None => {
                return Ok(PluginResult::Assertion(AssertionResult::Error(
                    "trailer: argument must be a string".to_string(),
                )));
            }
        };

        if let Some(trailers) = context.trailers {
            if trailers.contains_key(&key) {
                Ok(PluginResult::Assertion(AssertionResult::Pass))
            } else {
                Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                    "Trailer '{}' not found",
                    key
                ))))
            }
        } else {
            Ok(PluginResult::Assertion(AssertionResult::Error(
                "Trailers not available in this context".to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_trailer_plugin_name() {
        let plugin = TrailerPlugin;
        assert_eq!(plugin.name(), "trailer");
    }

    #[test]
    fn test_trailer_plugin_exists() {
        let plugin = TrailerPlugin;
        let mut trailers = HashMap::new();
        trailers.insert("x-custom".to_string(), "value".to_string());
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: Some(&trailers),
        };
        let result = plugin.execute(&[Value::String("x-custom".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_trailer_plugin_not_exists() {
        let plugin = TrailerPlugin;
        let mut trailers = HashMap::new();
        trailers.insert("x-custom".to_string(), "value".to_string());
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: Some(&trailers),
        };
        let result = plugin.execute(&[Value::String("x-other".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_trailer_plugin_no_trailers() {
        let plugin = TrailerPlugin;
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        };
        let result = plugin.execute(&[Value::String("x-custom".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Error(msg)) = result.unwrap() {
            assert!(msg.contains("Trailers not available"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn test_trailer_plugin_no_args() {
        let plugin = TrailerPlugin;
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
}
