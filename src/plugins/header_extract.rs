// Header plugin for EXTRACT section
// Extracts values from gRPC metadata headers

use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

/// Header plugin - extracts header values
#[derive(Debug, Clone, Default)]
pub struct HeaderExtractPlugin;

impl Plugin for HeaderExtractPlugin {
    fn name(&self) -> &'static str {
        "@header"
    }

    fn description(&self) -> &'static str {
        "Extract value from gRPC metadata headers"
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
                "@header requires 1 argument: the header name",
            )));
        }

        if args.len() > 1 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@header accepts only 1 argument",
            )));
        }

        // Get header name
        let header_name = match &args[0] {
            Value::String(name) => name.to_lowercase(),
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@header argument must be a string",
                )));
            }
        };

        // Get headers from context
        let headers = match context.headers {
            Some(h) => h,
            None => {
                return Ok(PluginResult::Value(Value::Null));
            }
        };

        // Case-insensitive header lookup
        let value = headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == header_name)
            .map(|(_, v)| v.clone());

        match value {
            Some(v) => Ok(PluginResult::Value(Value::String(v))),
            None => Ok(PluginResult::Value(Value::Null)),
        }
    }
}

/// HasHeader plugin - checks if header exists (returns boolean for assertions)
#[derive(Debug, Clone, Default)]
pub struct HasHeaderPlugin;

impl Plugin for HasHeaderPlugin {
    fn name(&self) -> &'static str {
        "has_header"
    }

    fn description(&self) -> &'static str {
        "Check if gRPC metadata header exists (returns true/false)"
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
        // Check argument count
        if args.is_empty() {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@has_header requires 1 argument: the header name",
            )));
        }

        if args.len() > 1 {
            return Ok(PluginResult::Assertion(AssertionResult::fail(
                "@has_header accepts only 1 argument",
            )));
        }

        // Get header name
        let header_name = match &args[0] {
            Value::String(name) => name.to_lowercase(),
            _ => {
                return Ok(PluginResult::Assertion(AssertionResult::fail(
                    "@has_header argument must be a string",
                )));
            }
        };

        // Get headers from context
        let headers = match context.headers {
            Some(h) => h,
            None => {
                return Ok(PluginResult::Value(Value::Bool(false)));
            }
        };

        // Case-insensitive header lookup
        let exists = headers.iter().any(|(k, _)| k.to_lowercase() == header_name);

        Ok(PluginResult::Value(Value::Bool(exists)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;

    static HEADERS: Lazy<HashMap<String, String>> = Lazy::new(|| {
        let mut h = HashMap::new();
        h.insert("content-type".to_string(), "application/json".to_string());
        h.insert("authorization".to_string(), "Bearer token123".to_string());
        h.insert("x-request-id".to_string(), "req-456".to_string());
        h
    });

    fn create_context_with_headers() -> PluginContext<'static> {
        PluginContext {
            response: &Value::Null,
            headers: Some(&HEADERS),
            trailers: None,
        }
    }

    fn create_context_no_headers() -> PluginContext<'static> {
        PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        }
    }

    #[test]
    fn test_header_plugin_exists() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("authorization".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "Bearer token123"));
    }

    #[test]
    fn test_header_plugin_case_insensitive() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        // Test different case variations
        let result1 = plugin
            .execute(&[Value::String("Authorization".to_string())], &context)
            .unwrap();

        let result2 = plugin
            .execute(&[Value::String("AUTHORIZATION".to_string())], &context)
            .unwrap();

        let result3 = plugin
            .execute(&[Value::String("authorization".to_string())], &context)
            .unwrap();

        assert!(matches!(result1, PluginResult::Value(Value::String(s)) if s == "Bearer token123"));
        assert!(matches!(result2, PluginResult::Value(Value::String(s)) if s == "Bearer token123"));
        assert!(matches!(result3, PluginResult::Value(Value::String(s)) if s == "Bearer token123"));
    }

    #[test]
    fn test_header_plugin_not_found() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("x-nonexistent".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Null)));
    }

    #[test]
    fn test_header_plugin_no_headers() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_no_headers();

        let result = plugin
            .execute(&[Value::String("authorization".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Null)));
    }

    #[test]
    fn test_header_plugin_no_args() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin.execute(&[], &context).unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_header_plugin_too_many_args() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(
                &[
                    Value::String("auth".to_string()),
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
    fn test_header_plugin_wrong_type() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::Number(123.into())], &context)
            .unwrap();

        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    fn test_header_plugin_x_request_id() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("x-request-id".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "req-456"));
    }

    #[test]
    fn test_header_plugin_content_type() {
        let plugin = HeaderExtractPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("content-type".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::String(s)) if s == "application/json"));
    }

    #[test]
    fn test_header_plugin_name() {
        let plugin = HeaderExtractPlugin;
        assert_eq!(plugin.name(), "@header");
    }

    #[test]
    fn test_header_plugin_description() {
        let plugin = HeaderExtractPlugin;
        assert!(plugin.description().contains("header"));
    }

    #[test]
    fn test_has_header_plugin_exists() {
        let plugin = HasHeaderPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("authorization".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_has_header_plugin_not_found() {
        let plugin = HasHeaderPlugin;
        let context = create_context_with_headers();

        let result = plugin
            .execute(&[Value::String("x-nonexistent".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_has_header_plugin_no_headers() {
        let plugin = HasHeaderPlugin;
        let context = create_context_no_headers();

        let result = plugin
            .execute(&[Value::String("authorization".to_string())], &context)
            .unwrap();

        assert!(matches!(result, PluginResult::Value(Value::Bool(false))));
    }

    #[test]
    fn test_has_header_plugin_case_insensitive() {
        let plugin = HasHeaderPlugin;
        let context = create_context_with_headers();

        let result1 = plugin
            .execute(&[Value::String("Authorization".to_string())], &context)
            .unwrap();

        let result2 = plugin
            .execute(&[Value::String("AUTHORIZATION".to_string())], &context)
            .unwrap();

        assert!(matches!(result1, PluginResult::Value(Value::Bool(true))));
        assert!(matches!(result2, PluginResult::Value(Value::Bool(true))));
    }

    #[test]
    fn test_has_header_plugin_name() {
        let plugin = HasHeaderPlugin;
        assert_eq!(plugin.name(), "has_header");
    }
}
