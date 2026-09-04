#[macro_export]
macro_rules! define_validation_plugin {
    (
        $(#[$attr:meta])*
        struct $name:ident {
            name: $name_str:expr,
            description: $desc_str:expr,
            validator: $validator:expr,
        }
    ) => {
        $crate::define_validation_plugin! {
            $(#[$attr])*
            struct $name {
                name: $name_str,
                description: $desc_str,
                invalid_label: $name_str,
                type_label: $name_str,
                validator: $validator,
            }
        }
    };
    (
        $(#[$attr:meta])*
        struct $name:ident {
            name: $name_str:expr,
            description: $desc_str:expr,
            invalid_label: $invalid_label:expr,
            type_label: $type_label:expr,
            validator: $validator:expr,
        }
    ) => {
        $(#[$attr])*
        pub struct $name;

        impl Plugin for $name {
            fn name(&self) -> &str {
                $name_str
            }

            fn description(&self) -> &str {
                $desc_str
            }

            fn signature(&self) -> PluginSignature {
                PluginSignature {
                    return_type: TypeInfo::Bool,
                    arg_types: &[ArgTypeInfo {
                        expected: TypeInfo::String,
                        required: true,
                        default: None,
                    }],
                    purity: PluginPurity::Pure,
                    deterministic: true,
                    idempotent: true,
                    safe_for_rewrite: true,
                    arg_names: &["value"],
                    replacement: None,
                }
            }

            fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
                if args.len() != 1 {
                    return Ok(PluginResult::Assertion(AssertionResult::Error(
                        format!("{}: expects exactly 1 argument", $name_str),
                    )));
                }

                let arg = &args[0];

                match arg.as_str() {
                    Some(s) => {
                        if ($validator)(s) {
                            Ok(PluginResult::Assertion(AssertionResult::Pass))
                        } else {
                            Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                                "Expected valid {}, got '{}'",
                                $invalid_label, s
                            ))))
                        }
                    }
                    None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected string for {} check, got {:?}",
                        $type_label, arg
                    )))),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! define_metadata_extract_plugin {
    (
        $(#[$attr:meta])*
        struct $name:ident {
            name: $name_str:expr,
            description: $desc_str:expr,
            accessor: $accessor:expr,
        }
    ) => {
        $(#[$attr])*
        pub struct $name;

        impl Plugin for $name {
            fn name(&self) -> &str {
                $name_str
            }

            fn description(&self) -> &str {
                $desc_str
            }

            fn signature(&self) -> PluginSignature {
                PluginSignature {
                    return_type: TypeInfo::String,
                    arg_types: &[ArgTypeInfo {
                        expected: TypeInfo::String,
                        required: true,
                        default: None,
                    }],
                    purity: PluginPurity::ContextDependent,
                    deterministic: true,
                    idempotent: true,
                    safe_for_rewrite: false,
                    arg_names: &["name"],
                    replacement: None,
                }
            }

            fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
                if args.is_empty() {
                    return Ok(PluginResult::Assertion(AssertionResult::fail(
                        format!("{} requires 1 argument: the {}", $name_str, $name_str),
                    )));
                }

                let arg = &args[0];
                let key = match arg.as_str() {
                    Some(s) => s,
                    None => {
                        return Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                            "Expected string for {} name, got {:?}",
                            $name_str, arg
                        ))))
                    }
                };

                let value = ($accessor)(context).and_then(|map| map.get(key).cloned());

                match value {
                    Some(v) => Ok(PluginResult::Value(Value::String(v))),
                    None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "No {} found for key '{}'",
                        $name_str, key
                    )))),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::{
        ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
    };
    use anyhow::Result;
    use apif_assert::engine::AssertionResult;
    use serde_json::Value;
    use std::collections::HashMap;

    fn header_accessor<'a>(ctx: &PluginContext<'a>) -> Option<&'a HashMap<String, String>> {
        ctx.headers
    }

    #[test]
    fn validation_plugin_failure_messages_are_exact() {
        let ctx = PluginContext::new(&Value::Null);
        let cases: &[(&dyn Plugin, &str, &str)] = &[
            (
                &crate::uuid::UuidPlugin,
                "Expected valid UUID, got 'nope'",
                "Expected string for UUID check, got Number(1)",
            ),
            (
                &crate::email::EmailPlugin,
                "Expected valid email, got 'nope'",
                "Expected string for email check, got Number(1)",
            ),
            (
                &crate::ip::IpPlugin,
                "Expected valid IP address, got 'nope'",
                "Expected string for IP check, got Number(1)",
            ),
            (
                &crate::url::UrlPlugin,
                "Expected valid URL, got 'nope'",
                "Expected string for URL check, got Number(1)",
            ),
            (
                &crate::timestamp::TimestampPlugin,
                "Expected valid RFC3339 timestamp, got 'nope'",
                "Expected string for timestamp check, got Number(1)",
            ),
        ];

        for (plugin, invalid_msg, type_msg) in cases {
            let got = plugin
                .execute(&[Value::String("nope".into())], &ctx)
                .unwrap();
            match got {
                PluginResult::Assertion(AssertionResult::Fail { message, .. }) => {
                    assert_eq!(&message, invalid_msg, "plugin {}", plugin.name())
                }
                other => panic!("{}: expected Fail, got {other:?}", plugin.name()),
            }

            let got = plugin.execute(&[Value::Number(1.into())], &ctx).unwrap();
            match got {
                PluginResult::Assertion(AssertionResult::Fail { message, .. }) => {
                    assert_eq!(&message, type_msg, "plugin {}", plugin.name())
                }
                other => panic!("{}: expected Fail, got {other:?}", plugin.name()),
            }

            let got = plugin.execute(&[], &ctx).unwrap();
            match got {
                PluginResult::Assertion(AssertionResult::Error(msg)) => assert_eq!(
                    msg,
                    format!("{}: expects exactly 1 argument", plugin.name())
                ),
                other => panic!("{}: expected Error, got {other:?}", plugin.name()),
            }
        }
    }

    crate::define_validation_plugin! {
        struct NonEmptyPlugin {
            name: "non_empty",
            description: "checks a string is non-empty",
            validator: |s: &str| !s.is_empty(),
        }
    }

    crate::define_metadata_extract_plugin! {
        struct MetaHeaderPlugin {
            name: "meta_header",
            description: "extracts a header value",
            accessor: header_accessor,
        }
    }

    #[test]
    fn validation_macro_compiles_and_runs() {
        let ctx = PluginContext::new(&Value::Null);

        let pass = NonEmptyPlugin
            .execute(&[Value::String("x".into())], &ctx)
            .unwrap();
        assert!(matches!(
            pass,
            PluginResult::Assertion(AssertionResult::Pass)
        ));

        let fail = NonEmptyPlugin
            .execute(&[Value::String(String::new())], &ctx)
            .unwrap();
        assert!(matches!(
            fail,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));

        assert_eq!(NonEmptyPlugin.signature().return_type, TypeInfo::Bool);
        assert_eq!(NonEmptyPlugin.name(), "non_empty");
    }

    #[test]
    fn metadata_macro_compiles_and_runs() {
        let mut headers = HashMap::new();
        headers.insert("x-id".to_string(), "42".to_string());
        let ctx = PluginContext::new(&Value::Null).with_headers(Some(&headers));

        let found = MetaHeaderPlugin
            .execute(&[Value::String("x-id".into())], &ctx)
            .unwrap();
        assert!(matches!(found, PluginResult::Value(Value::String(s)) if s == "42"));

        let missing = MetaHeaderPlugin
            .execute(&[Value::String("nope".into())], &ctx)
            .unwrap();
        assert!(matches!(
            missing,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));

        assert_eq!(MetaHeaderPlugin.signature().return_type, TypeInfo::String);
    }
}
