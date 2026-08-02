use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

crate::define_validation_plugin! {
    struct UuidPlugin {
        name: "uuid",
        description: "Validates if the provided value is a valid UUID string",
        invalid_label: "UUID",
        type_label: "UUID",
        validator: |s: &str| uuid::Uuid::parse_str(s).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn uuid_plugin_name() {
        let plugin = UuidPlugin;
        assert_eq!(plugin.name(), "uuid");
    }

    #[test]
    fn uuid_plugin_description() {
        let plugin = UuidPlugin;
        assert!(plugin.description().contains("UUID"));
    }

    #[test]
    fn uuid_plugin_valid_uuid() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::String(
                "550e8400-e29b-41d4-a716-446655440000".to_string(),
            )],
            &context,
        );
        assert_eq!(
            result.expect("uuid plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn uuid_plugin_invalid_uuid() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-uuid".to_string())], &context);
        let got = result.expect("uuid plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn uuid_plugin_wrong_type() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        let got = result.expect("uuid plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn uuid_plugin_no_args() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("uuid plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn uuid_plugin_too_many_args() {
        let plugin = UuidPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("test".to_string()),
                Value::String("test2".to_string()),
            ],
            &context,
        );
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("uuid plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }
}
