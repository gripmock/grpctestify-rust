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
                    purity: PluginPurity::ContextDependent,
                    deterministic: true,
                    idempotent: true,
                    safe_for_rewrite: false,
                    arg_names: &["name"],
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
