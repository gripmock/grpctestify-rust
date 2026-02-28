// Trailer plugin for EXTRACT section
// Extracts values from gRPC metadata trailers

use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

/// Trailer plugin - extracts trailer values
#[derive(Debug, Clone, Default)]
pub struct TrailerExtractPlugin;

impl Plugin for TrailerExtractPlugin {
    fn name(&self) -> &'static str {
        "@trailer"
    }

    fn description(&self) -> &'static str {
        "Extract value from gRPC metadata trailers"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::String,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["name"],
        }
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        // Check argument count
        if args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@trailer requires 1 argument: the trailer name",
            )));
        }

        if args.len() > 1 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@trailer accepts only 1 argument",
            )));
        }

        // Get trailer name
        let trailer_name = match &args[0] {
            Value::String(name) => name.to_lowercase(),
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@trailer argument must be a string",
                )));
            }
        };

        // Get trailers from context
        let trailers = match context.trailers {
            Some(t) => t,
            None => {
                return Ok(PluginResult::Value(Value::Null));
            }
        };

        // Case-insensitive trailer lookup
        let value = trailers
            .iter()
            .find(|(k, _)| k.to_lowercase() == trailer_name)
            .map(|(_, v)| v.clone());

        match value {
            Some(v) => Ok(PluginResult::Value(Value::String(v))),
            None => Ok(PluginResult::Value(Value::Null)),
        }
    }
}

/// HasTrailer plugin - checks if trailer exists (returns boolean for assertions)
#[derive(Debug, Clone, Default)]
pub struct HasTrailerPlugin;

impl Plugin for HasTrailerPlugin {
    fn name(&self) -> &'static str {
        "has_trailer"
    }

    fn description(&self) -> &'static str {
        "Check if gRPC metadata trailer exists (returns true/false)"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Boolean,
            purity: PluginPurity::ContextDependent,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["name"],
        }
    }

    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@has_trailer requires 1 argument: the trailer name",
            )));
        }

        if args.len() > 1 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@has_trailer accepts only 1 argument",
            )));
        }

        let trailer_name = match &args[0] {
            Value::String(name) => name.to_lowercase(),
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@has_trailer argument must be a string",
                )));
            }
        };

        let trailers = match context.trailers {
            Some(t) => t,
            None => return Ok(PluginResult::Value(Value::Bool(false))),
        };

        let exists = trailers
            .iter()
            .any(|(k, _)| k.to_lowercase() == trailer_name);

        Ok(PluginResult::Value(Value::Bool(exists)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;

    static TRAILERS: Lazy<HashMap<String, String>> = Lazy::new(|| {
        let mut t = HashMap::new();
        t.insert("x-status".to_string(), "success".to_string());
        t.insert("x-checksum".to_string(), "abc123".to_string());
        t.insert("x-processing-time-ms".to_string(), "42".to_string());
        t
    });

    fn create_context_with_trailers() -> PluginContext<'static> {
        PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: Some(&TRAILERS),
        }
    }

    fn create_context_no_trailers() -> PluginContext<'static> {
        PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        }
    }

    #[test]
    fn test_trailer_plugin_exists() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::String("x-status".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "success"));
    }

    #[test]
    fn test_trailer_plugin_case_insensitive() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result1 = plugin
            .execute(&[Value::String("X-Status".to_string())], &context)
            .unwrap();

        let result2 = plugin
            .execute(&[Value::String("x-status".to_string())], &context)
            .unwrap();

        assert!(matches!(result1, PluginResult::Value(Value::String(s)) if s == "success"));
        assert!(matches!(result2, PluginResult::Value(Value::String(s)) if s == "success"));
    }

    #[test]
    fn test_trailer_plugin_not_found() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::String("x-nonexistent".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Null)));
    }

    #[test]
    fn test_trailer_plugin_no_trailers() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_no_trailers();

        let result = plugin
            .execute(&[Value::String("x-status".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Null)));
    }

    #[test]
    fn test_trailer_plugin_no_args() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin.execute(&[], &context).unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_trailer_plugin_too_many_args() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(
                &[
                    Value::String("status".to_string()),
                    Value::String("extra".to_string()),
                ],
                &context,
            )
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_trailer_plugin_wrong_type() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::Number(123.into())], &context)
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_trailer_plugin_checksum() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::String("x-checksum".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "abc123"));
    }

    #[test]
    fn test_trailer_plugin_processing_time() {
        let plugin = TrailerExtractPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(
                &[Value::String("x-processing-time-ms".to_string())],
                &context,
            )
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "42"));
    }

    #[test]
    fn test_trailer_plugin_name() {
        let plugin = TrailerExtractPlugin;
        assert_eq!(plugin.name(), "@trailer");
    }

    #[test]
    fn test_trailer_plugin_description() {
        let plugin = TrailerExtractPlugin;
        assert!(plugin.description().contains("trailer"));
    }

    #[test]
    fn test_has_trailer_plugin_exists() {
        let plugin = HasTrailerPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::String("x-status".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_has_trailer_plugin_not_found() {
        let plugin = HasTrailerPlugin;
        let context = create_context_with_trailers();

        let result = plugin
            .execute(&[Value::String("x-missing".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_has_trailer_plugin_name() {
        let plugin = HasTrailerPlugin;
        assert_eq!(plugin.name(), "has_trailer");
    }
}
