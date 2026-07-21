/// Macro to define a simple validation plugin that validates a single string argument
/// using a closure-based validation function.
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
                                $name_str, s
                            ))))
                        }
                    }
                    None => Ok(PluginResult::Assertion(AssertionResult::fail(format!(
                        "Expected string for {} check, got {:?}",
                        $name_str, arg
                    )))),
                }
            }
        }
    };
}

/// Macro to define a metadata extraction plugin (header/trailer)
/// Extracts values from gRPC metadata using a accessor function.
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
    // Bring every identifier the macros expand to into scope so the generated
    // code compiles at the macro call site.
    use crate::{
        ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
    };
    use anyhow::Result;
    use apif_assert::engine::AssertionResult;
    use serde_json::Value;
    use std::collections::HashMap;

    // A free function (rather than a closure) so the returned reference can be
    // tied to the context's lifetime parameter.
    fn header_accessor<'a>(ctx: &PluginContext<'a>) -> Option<&'a HashMap<String, String>> {
        ctx.headers
    }

    // Instantiate the exported macros to prove they compile and behave.
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
