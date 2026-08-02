use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

crate::define_validation_plugin! {
    struct EmailPlugin {
        name: "email",
        description: "Validates if the provided value is a valid email address",
        invalid_label: "email",
        type_label: "email",
        validator: |s: &str| email_address::EmailAddress::is_valid(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn email_plugin_name() {
        let plugin = EmailPlugin;
        assert_eq!(plugin.name(), "email");
    }

    #[test]
    fn email_plugin_valid_email() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("test@example.com".to_string())], &context);
        assert_eq!(
            result.expect("email plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn email_plugin_invalid_email() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-an-email".to_string())], &context);
        let got = result.expect("email plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn email_plugin_wrong_type() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        let got = result.expect("email plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn email_plugin_no_args() {
        let plugin = EmailPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("email plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }
}
