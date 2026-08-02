use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

crate::define_validation_plugin! {
    struct TimestampPlugin {
        name: "timestamp",
        description: "Validates if the provided value is a valid RFC3339 timestamp",
        invalid_label: "RFC3339 timestamp",
        type_label: "timestamp",
        validator: |s: &str| chrono::DateTime::parse_from_rfc3339(s).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn timestamp_plugin_name() {
        let plugin = TimestampPlugin;
        assert_eq!(plugin.name(), "timestamp");
    }

    #[test]
    fn timestamp_plugin_valid() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::String("2024-01-15T10:30:00Z".to_string())],
            &context,
        );
        assert_eq!(
            result.expect("timestamp plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn timestamp_plugin_invalid() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-timestamp".to_string())], &context);
        let got = result.expect("timestamp plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn timestamp_plugin_no_args() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("timestamp plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn timestamp_plugin_too_many_args() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("2024-01-15T10:30:00Z".to_string()),
                Value::String("extra".to_string()),
            ],
            &context,
        );
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("timestamp plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn timestamp_plugin_wrong_type() {
        let plugin = TimestampPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::Number(serde_json::Number::from(1234567890))],
            &context,
        );
        let got = result.expect("timestamp plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn timestamp_plugin_description() {
        let plugin = TimestampPlugin;
        assert!(plugin.description().contains("RFC3339"));
    }

    #[test]
    fn timestamp_plugin_signature() {
        let plugin = TimestampPlugin;
        let sig = plugin.signature();
        assert_eq!(sig.arg_names, &["value"]);
        assert!(sig.safe_for_rewrite);
    }
}
