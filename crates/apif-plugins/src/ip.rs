use anyhow::Result;
use serde_json::Value;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};
use apif_assert::engine::AssertionResult;

crate::define_validation_plugin! {
    struct IpPlugin {
        name: "ip",
        description: "Validates if the provided value is a valid IP address",
        invalid_label: "IP address",
        type_label: "IP",
        validator: |s: &str| s.parse::<std::net::IpAddr>().is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_context() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    #[test]
    fn ip_plugin_name() {
        let plugin = IpPlugin;
        assert_eq!(plugin.name(), "ip");
    }

    #[test]
    fn ip_plugin_valid_ipv4() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("192.168.1.1".to_string())], &context);
        assert_eq!(
            result.expect("ip plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn ip_plugin_valid_ipv6() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("::1".to_string())], &context);
        assert_eq!(
            result.expect("ip plugin must execute"),
            PluginResult::Assertion(AssertionResult::Pass)
        );
    }

    #[test]
    fn ip_plugin_invalid_ip() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-an-ip".to_string())], &context);
        let got = result.expect("ip plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn ip_plugin_no_args() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[], &context);
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("ip plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn ip_plugin_too_many_args() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(
            &[
                Value::String("192.168.1.1".to_string()),
                Value::String("10.0.0.1".to_string()),
            ],
            &context,
        );
        if let PluginResult::Assertion(AssertionResult::Error(msg)) =
            result.expect("ip plugin must execute")
        {
            assert!(msg.contains("1 argument"));
        } else {
            panic!("Expected Error assertion result");
        }
    }

    #[test]
    fn ip_plugin_wrong_type() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::Number(serde_json::Number::from(123))], &context);
        let got = result.expect("ip plugin must execute");
        assert!(
            matches!(got, PluginResult::Assertion(AssertionResult::Fail { .. })),
            "expected a Fail assertion, got {got:?}"
        );
    }

    #[test]
    fn ip_plugin_description() {
        let plugin = IpPlugin;
        assert!(plugin.description().contains("IP"));
    }

    #[test]
    fn ip_plugin_signature() {
        let plugin = IpPlugin;
        let sig = plugin.signature();
        assert_eq!(sig.arg_names, &["value"]);
        assert!(sig.safe_for_rewrite);
    }
}
