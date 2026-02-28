use anyhow::Result;
use serde_json::Value;

use crate::assert::engine::AssertionResult;
use crate::plugins::{
    Plugin, PluginContext, PluginPurity, PluginResult, PluginReturnKind, PluginSignature,
};

pub struct IpPlugin;

impl Plugin for IpPlugin {
    fn name(&self) -> &str {
        "ip"
    }

    fn description(&self) -> &str {
        "Validates if the provided value is a valid IP address"
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_kind: PluginReturnKind::Boolean,
            purity: PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
            arg_names: &["value"],
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if args.len() != 1 {
            return Ok(PluginResult::Assertion(AssertionResult::Error(
                "ip: expects exactly 1 argument".to_string(),
            )));
        }

        let arg = &args[0];

        match arg.as_str() {
            Some(s) => {
                if s.parse::<std::net::IpAddr>().is_ok() {
                    Ok(PluginResult::Assertion(AssertionResult::Pass))
                } else {
                    Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected valid IP address, got '{}'",
                        s
                    ))))
                }
            }
            None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                "Expected string for IP check, got {:?}",
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
    fn test_ip_plugin_name() {
        let plugin = IpPlugin;
        assert_eq!(plugin.name(), "ip");
    }

    #[test]
    fn test_ip_plugin_valid_ipv4() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("192.168.1.1".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_ip_plugin_valid_ipv6() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("::1".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Pass) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Pass assertion result");
        }
    }

    #[test]
    fn test_ip_plugin_invalid_ip() {
        let plugin = IpPlugin;
        let context = create_context();
        let result = plugin.execute(&[Value::String("not-an-ip".to_string())], &context);
        assert!(result.is_ok());
        if let PluginResult::Assertion(AssertionResult::Fail { .. }) = result.unwrap() {
            // Pass
        } else {
            panic!("Expected Fail assertion result");
        }
    }

    #[test]
    fn test_ip_plugin_no_args() {
        let plugin = IpPlugin;
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
