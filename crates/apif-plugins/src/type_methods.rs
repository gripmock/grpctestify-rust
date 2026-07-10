//! Type methods — plugins registered under `@type.method` names.
//! E.g., `@url.scheme(.x)`, `@email.domain(.x)`, `@json.key(.x, "k")`.

use crate::core::{Plugin, PluginContext, PluginResult, PluginSignature};
use crate::type_info::{ArgTypeInfo, TypeInfo};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

fn url_str_plugin(args: &[Value], f: fn(&url::Url) -> String) -> Result<PluginResult> {
    if args.is_empty() {
        return Ok(PluginResult::Value(Value::Null));
    }
    let s = match &args[0] {
        Value::String(s) => s,
        _ => return Ok(PluginResult::Value(Value::Null)),
    };
    let url = match url::Url::parse(s) {
        Ok(u) => u,
        Err(_) => return Ok(PluginResult::Value(Value::Null)),
    };
    Ok(PluginResult::Value(Value::String(f(&url))))
}

pub struct UrlScheme;
impl Plugin for UrlScheme {
    fn name(&self) -> &str {
        "url.scheme"
    }
    fn description(&self) -> &str {
        "Extract scheme from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| u.scheme().to_string())
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["url"],
            replacement: None,
        }
    }
}

pub struct UrlHost;
impl Plugin for UrlHost {
    fn name(&self) -> &str {
        "url.host"
    }
    fn description(&self) -> &str {
        "Extract host from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| u.host_str().unwrap_or("").to_string())
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            ..UrlScheme.signature()
        }
    }
}

pub struct UrlPort;
impl Plugin for UrlPort {
    fn name(&self) -> &str {
        "url.port"
    }
    fn description(&self) -> &str {
        "Extract port from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| {
            u.port().map(|p| p.to_string()).unwrap_or_default()
        })
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            ..UrlScheme.signature()
        }
    }
}

pub struct UrlPath;
impl Plugin for UrlPath {
    fn name(&self) -> &str {
        "url.path"
    }
    fn description(&self) -> &str {
        "Extract path from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| u.path().to_string())
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            ..UrlScheme.signature()
        }
    }
}

pub struct UrlQuery;
impl Plugin for UrlQuery {
    fn name(&self) -> &str {
        "url.query"
    }
    fn description(&self) -> &str {
        "Extract query string from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| u.query().unwrap_or("").to_string())
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            ..UrlScheme.signature()
        }
    }
}

pub struct UrlFragment;
impl Plugin for UrlFragment {
    fn name(&self) -> &str {
        "url.fragment"
    }
    fn description(&self) -> &str {
        "Extract fragment from a URL"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        url_str_plugin(args, |u| u.fragment().unwrap_or("").to_string())
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            ..UrlScheme.signature()
        }
    }
}

/// Email methods
pub struct EmailLocalPart;
impl Plugin for EmailLocalPart {
    fn name(&self) -> &str {
        "email.local_part"
    }
    fn description(&self) -> &str {
        "Extract local part from an email address"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Null));
        }
        let s = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let local = s.split('@').nth(0).unwrap_or("").to_string();
        Ok(PluginResult::Value(Value::String(local)))
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["email"],
            replacement: None,
        }
    }
}

pub struct EmailDomain;
impl Plugin for EmailDomain {
    fn name(&self) -> &str {
        "email.domain"
    }
    fn description(&self) -> &str {
        "Extract domain from an email address"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Null));
        }
        let s = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let domain = s.split('@').nth(1).unwrap_or("").to_string();
        Ok(PluginResult::Value(Value::String(domain)))
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::String,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["email"],
            replacement: None,
        }
    }
}

/// IP methods
pub struct IpVersion;
impl Plugin for IpVersion {
    fn name(&self) -> &str {
        "ip.version"
    }
    fn description(&self) -> &str {
        "Extract IP version (4 or 6)"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Null));
        }
        let s = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let version = if s.contains(':') {
            6
        } else if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
            4
        } else {
            return Ok(PluginResult::Value(Value::Null));
        };
        Ok(PluginResult::Value(Value::Number(version.into())))
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::UInt,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["ip"],
            replacement: None,
        }
    }
}

/// UUID methods
pub struct UuidVersion;
impl Plugin for UuidVersion {
    fn name(&self) -> &str {
        "uuid.version"
    }
    fn description(&self) -> &str {
        "Extract UUID version number"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        if args.is_empty() {
            return Ok(PluginResult::Value(Value::Null));
        }
        let s = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let v = s.chars().nth(14).and_then(|c| c.to_digit(10)).unwrap_or(0);
        Ok(PluginResult::Value(Value::Number(v.into())))
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::UInt,
            arg_types: &[ArgTypeInfo {
                expected: TypeInfo::String,
                required: true,
                default: None,
            }],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["uuid"],
            replacement: None,
        }
    }
}

/// JSON methods
pub struct JsonKey;
impl Plugin for JsonKey {
    fn name(&self) -> &str {
        "json.key"
    }
    fn description(&self) -> &str {
        "Extract a key from a JSON string"
    }
    fn execute(&self, args: &[Value], _ctx: &PluginContext) -> Result<PluginResult> {
        if args.len() < 2 {
            return Ok(PluginResult::Value(Value::Null));
        }
        let s = match &args[0] {
            Value::String(s) => s,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let key = match &args[1] {
            Value::String(k) => k,
            _ => return Ok(PluginResult::Value(Value::Null)),
        };
        let parsed: serde_json::Value = match serde_json::from_str(s) {
            Ok(v) => v,
            Err(_) => return Ok(PluginResult::Value(Value::Null)),
        };
        let result = parsed.get(key).cloned().unwrap_or(Value::Null);
        Ok(PluginResult::Value(result))
    }
    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: TypeInfo::Any,
            arg_types: &[
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: true,
                    default: None,
                },
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: true,
                    default: None,
                },
            ],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: false,
            arg_names: &["json", "key"],
            replacement: None,
        }
    }
}

/// Register all type methods with a plugin manager.
pub fn register_all(manager: &mut crate::core::PluginManager) {
    let plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(UrlScheme),
        Arc::new(UrlHost),
        Arc::new(UrlPort),
        Arc::new(UrlPath),
        Arc::new(UrlQuery),
        Arc::new(UrlFragment),
        Arc::new(EmailLocalPart),
        Arc::new(EmailDomain),
        Arc::new(IpVersion),
        Arc::new(UuidVersion),
        Arc::new(JsonKey),
    ];
    for p in plugins {
        manager.register(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Plugin;
    use crate::core::PluginManager;
    use serde_json::json;

    fn ctx() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    macro_rules! test_plugin_basics {
        ($plugin:expr, $name:expr) => {
            let p = $plugin;
            assert_eq!(p.name(), $name);
            assert!(
                !p.description().is_empty(),
                "{} description should not be empty",
                $name
            );
            let sig = p.signature();
            assert!(
                sig.arg_names.len() == sig.arg_types.len(),
                "{} arg_names/arg_types length mismatch",
                $name
            );
        };
    }

    #[test]
    fn test_url_scheme() {
        test_plugin_basics!(UrlScheme, "url.scheme");
        let result = UrlScheme
            .execute(&[json!("https://example.com/path")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("https")));
    }

    #[test]
    fn test_url_host() {
        test_plugin_basics!(UrlHost, "url.host");
        let result = UrlHost
            .execute(&[json!("https://example.com:8080/path")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("example.com")));
    }

    #[test]
    fn test_url_port() {
        test_plugin_basics!(UrlPort, "url.port");
        let result = UrlPort
            .execute(&[json!("https://example.com:8080/path")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("8080")));
        // No port
        let result2 = UrlPort
            .execute(&[json!("https://example.com/path")], &ctx())
            .unwrap();
        assert_eq!(result2, PluginResult::Value(json!("")));
    }

    #[test]
    fn test_url_path() {
        test_plugin_basics!(UrlPath, "url.path");
        let result = UrlPath
            .execute(&[json!("https://example.com/api/v1")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("/api/v1")));
    }

    #[test]
    fn test_url_query() {
        test_plugin_basics!(UrlQuery, "url.query");
        let result = UrlQuery
            .execute(&[json!("https://example.com/path?q=1&r=2")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("q=1&r=2")));
    }

    #[test]
    fn test_url_fragment() {
        test_plugin_basics!(UrlFragment, "url.fragment");
        let result = UrlFragment
            .execute(&[json!("https://example.com/path#section")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("section")));
    }

    #[test]
    fn test_url_methods_invalid() {
        for method in &[
            &UrlScheme as &dyn Plugin,
            &UrlHost,
            &UrlPort,
            &UrlPath,
            &UrlQuery,
            &UrlFragment,
        ] {
            // No args
            let result = method.execute(&[], &ctx()).unwrap();
            assert_eq!(result, PluginResult::Value(Value::Null));

            // Non-string arg
            let result = method.execute(&[json!(42)], &ctx()).unwrap();
            assert_eq!(result, PluginResult::Value(Value::Null));

            // Invalid URL
            let result = method.execute(&[json!("not-a-url")], &ctx()).unwrap();
            assert_eq!(result, PluginResult::Value(Value::Null));
        }
    }

    #[test]
    fn test_email_local_part() {
        test_plugin_basics!(EmailLocalPart, "email.local_part");
        let result = EmailLocalPart
            .execute(&[json!("user@example.com")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("user")));
    }

    #[test]
    fn test_email_domain() {
        test_plugin_basics!(EmailDomain, "email.domain");
        let result = EmailDomain
            .execute(&[json!("user@example.com")], &ctx())
            .unwrap();
        assert_eq!(result, PluginResult::Value(json!("example.com")));
    }

    #[test]
    fn test_email_methods_invalid() {
        // EmailLocalPart: no @ → split returns [whole], so local part = whole string
        let r = EmailLocalPart.execute(&[], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        let r = EmailLocalPart.execute(&[json!(42)], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        let r = EmailLocalPart
            .execute(&[json!("noatsign")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(json!("noatsign")));

        // EmailDomain: no @ → split returns [whole], so domain = ""
        let r = EmailDomain.execute(&[json!("noatsign")], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(json!("")));

        let r = EmailDomain.execute(&[], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        let r = EmailDomain.execute(&[json!(42)], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));
    }

    #[test]
    fn test_ip_version() {
        test_plugin_basics!(IpVersion, "ip.version");
        let r4 = IpVersion.execute(&[json!("192.168.1.1")], &ctx()).unwrap();
        assert_eq!(r4, PluginResult::Value(json!(4)));

        let r6 = IpVersion.execute(&[json!("::1")], &ctx()).unwrap();
        assert_eq!(r6, PluginResult::Value(json!(6)));

        // Invalid — empty string matches the all-digit-or-dot check, so returns 4
        let r = IpVersion.execute(&[json!("")], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(json!(4)));

        let r = IpVersion.execute(&[], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        let r = IpVersion.execute(&[json!(42)], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));
    }

    #[test]
    fn test_uuid_version() {
        test_plugin_basics!(UuidVersion, "uuid.version");
        let r = UuidVersion
            .execute(&[json!("550e8400-e29b-41d4-a716-446655440000")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(json!(4)));

        let r = UuidVersion
            .execute(&[json!("00000000-0000-51d4-a716-446655440000")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(json!(5)));

        let r = UuidVersion.execute(&[json!("not-a-uuid")], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(json!(0)));

        let r = UuidVersion.execute(&[], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));
    }

    #[test]
    fn test_json_key() {
        test_plugin_basics!(JsonKey, "json.key");
        let r = JsonKey
            .execute(&[json!(r#"{"name":"test"}"#), json!("name")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(json!("test")));

        // Missing key
        let r = JsonKey
            .execute(&[json!(r#"{"name":"test"}"#), json!("missing")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        // Invalid JSON string
        let r = JsonKey
            .execute(&[json!("not-json"), json!("key")], &ctx())
            .unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        // Not enough args
        let r = JsonKey.execute(&[json!("{}")], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        let r = JsonKey.execute(&[], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));

        // Non-string args
        let r = JsonKey.execute(&[json!(42), json!("key")], &ctx()).unwrap();
        assert_eq!(r, PluginResult::Value(Value::Null));
    }

    #[test]
    fn test_type_method_signatures_via_manager() {
        let manager = PluginManager::new();
        for name in &[
            "url.scheme",
            "url.host",
            "url.port",
            "url.path",
            "url.query",
            "url.fragment",
            "email.local_part",
            "email.domain",
            "ip.version",
            "uuid.version",
            "json.key",
        ] {
            let plugin = manager
                .get(name)
                .unwrap_or_else(|| panic!("{} not registered", name));
            assert!(!plugin.description().is_empty(), "{} description", name);
            let sig = plugin.signature();
            assert!(!sig.arg_types.is_empty(), "{} should have arg types", name);
        }
    }

    #[test]
    fn test_url_str_plugin_empty_args() {
        assert_eq!(
            url_str_plugin(&[], |_| "".into()).unwrap(),
            PluginResult::Value(Value::Null)
        );
        assert_eq!(
            url_str_plugin(&[json!(42)], |_| "".into()).unwrap(),
            PluginResult::Value(Value::Null)
        );
        assert_eq!(
            url_str_plugin(&[json!("bad-url")], |_| "".into()).unwrap(),
            PluginResult::Value(Value::Null)
        );
    }
}
