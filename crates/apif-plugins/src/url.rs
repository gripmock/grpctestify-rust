use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

crate::define_validation_plugin! {
    struct UrlPlugin {
        name: "url",
        description: "Validates if the provided value is a valid URL",
        invalid_label: "URL",
        type_label: "URL",
        validator: |s: &str| url::Url::parse(s).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn url_plugin_name() {
        let plugin = UrlPlugin;
        assert_eq!(plugin.name(), "url");
    }

    #[test]
    fn url_plugin_valid_url() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[Value::String("https://example.com".to_string())],
            &context,
        );
        assert_eq!(
            result.expect("url plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn url_plugin_invalid_url() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-a-url".to_string())], &context);
        let got = result.expect("url plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn url_plugin_no_args() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("url plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn url_plugin_too_many_args() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("https://example.com".to_string()),
                Value::String("extra".to_string()),
            ],
            &context,
        );
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("url plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn url_plugin_wrong_type() {
        let plugin = UrlPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        let got = result.expect("url plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn url_plugin_description() {
        let plugin = UrlPlugin;
        assert!(plugin.description().contains("URL"));
    }

    #[test]
    fn url_plugin_signature() {
        let plugin = UrlPlugin;
        let sig = plugin.signature();
        assert_eq!(sig.arg_names, &["value"]);
        assert!(sig.safe_for_rewrite);
    }
}
