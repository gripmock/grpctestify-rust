//! Compact type system for plugin arguments and return values.

use serde::{Deserialize, Serialize};

/// Type information for plugin return values and assertion expressions.
/// Only 7 core types. Constrained strings (uuid, email, url, ip) are aliases
/// resolved to `String` by `parse_type_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeInfo {
    Bool,
    UInt,
    Number,
    String,
    /// Time/duration value (unix timestamp, ISO 8601, protobuf Timestamp/Duration).
    /// Supports ordering operators: `>`, `<`, `>=`, `<=`.
    Time,
    Json,
    Yaml,
    Any,
}

impl TypeInfo {
    pub fn is_numeric(&self) -> bool {
        matches!(self, TypeInfo::UInt | TypeInfo::Number)
    }

    pub fn is_stringy(&self) -> bool {
        matches!(self, TypeInfo::String)
    }

    pub fn is_temporal(&self) -> bool {
        matches!(self, TypeInfo::Time)
    }

    pub fn is_nullable(&self) -> bool {
        matches!(self, TypeInfo::Any)
    }

    pub fn parse_type_name(name: &str) -> Option<TypeInfo> {
        Some(match name {
            "bool" => TypeInfo::Bool,
            "uint" => TypeInfo::UInt,
            "number" => TypeInfo::Number,
            "string" | "uuid" | "email" | "url" | "ip" => TypeInfo::String,
            "time" | "timestamp" | "duration" => TypeInfo::Time,
            "json" => TypeInfo::Json,
            "yaml" => TypeInfo::Yaml,
            _ => return None,
        })
    }

    pub fn base_type(&self) -> TypeInfo {
        *self
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TypeInfo::Bool => "bool",
            TypeInfo::UInt => "uint",
            TypeInfo::Number => "number",
            TypeInfo::String => "string",
            TypeInfo::Time => "time",
            TypeInfo::Json => "json",
            TypeInfo::Yaml => "yaml",
            TypeInfo::Any => "any",
        }
    }

    pub fn supports_operator(&self, op: &str) -> (bool, Option<&'static str>) {
        match op {
            "==" | "!=" => (true, None),
            ">" | "<" | ">=" | "<=" => {
                if self.is_numeric() || self.is_temporal() {
                    (true, None)
                } else if self == &TypeInfo::Bool {
                    (
                        false,
                        Some("boolean values cannot be compared with <, >, <=, >="),
                    )
                } else if self.is_stringy() {
                    (
                        false,
                        Some(
                            "use 'contains', 'startsWith', or 'endsWith' for string comparison, not <, >, <=, >=",
                        ),
                    )
                } else {
                    (
                        false,
                        Some("this type does not support ordering comparisons"),
                    )
                }
            }
            "contains" | "startsWith" | "endsWith" | "matches" => {
                if self.is_stringy() {
                    (true, None)
                } else if self.is_numeric() || self.is_temporal() {
                    (
                        false,
                        Some(
                            "numeric and time values do not support string operators — use == or != instead",
                        ),
                    )
                } else if self == &TypeInfo::Bool {
                    (
                        false,
                        Some("boolean values do not support string operators"),
                    )
                } else {
                    (false, Some("this type does not support string operators"))
                }
            }
            _ => (false, Some("unknown operator")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgTypeInfo {
    pub expected: TypeInfo,
    pub required: bool,
    pub default: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct TypedPluginSignature {
    pub return_type: TypeInfo,
    pub arg_types: &'static [ArgTypeInfo],
    pub purity: crate::PluginPurity,
    pub deterministic: bool,
    pub idempotent: bool,
    pub safe_for_rewrite: bool,
}

impl TypedPluginSignature {
    pub fn valid_arg_count(&self, count: usize) -> bool {
        let required = self.arg_types.iter().filter(|a| a.required).count();
        let total = self.arg_types.len();
        count >= required && count <= total
    }

    pub fn arg_type(&self, idx: usize) -> Option<TypeInfo> {
        self.arg_types.get(idx).map(|a| a.expected)
    }
}

pub fn typed_plugin_signatures() -> std::collections::HashMap<String, TypedPluginSignature> {
    use crate::PluginPurity;
    let mut map = std::collections::HashMap::new();

    macro_rules! plugin_sig {
        ($name:expr, $return_type:expr, purity: $purity:expr, deterministic: $det:expr, idempotent: $idem:expr, rewrite: $rewrite:expr, args: [$($arg:expr),* $(,)?]) => {
            map.insert(
                $name.to_string(),
                TypedPluginSignature {
                    return_type: $return_type,
                    arg_types: &[$($arg),*],
                    purity: $purity,
                    deterministic: $det,
                    idempotent: $idem,
                    safe_for_rewrite: $rewrite,
                },
            );
        };
    }

    plugin_sig!("@uuid", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@email", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@ip", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@url", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@timestamp", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::Any, required: true, default: None }]);

    plugin_sig!("@regex", TypeInfo::Bool,
    purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: false,
    args: [
        ArgTypeInfo { expected: TypeInfo::String, required: true, default: None },
        ArgTypeInfo { expected: TypeInfo::String, required: true, default: None },
    ]);

    plugin_sig!("@len", TypeInfo::UInt,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::Any, required: true, default: None }]);

    plugin_sig!("@empty", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::Any, required: true, default: None }]);

    plugin_sig!("@header", TypeInfo::String,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@has_header", TypeInfo::Bool,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@trailer", TypeInfo::String,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@has_trailer", TypeInfo::Bool,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@env", TypeInfo::String,
    purity: PluginPurity::Impure, deterministic: false, idempotent: false, rewrite: false,
    args: [
        ArgTypeInfo { expected: TypeInfo::String, required: true, default: None },
        ArgTypeInfo { expected: TypeInfo::String, required: false, default: None },
    ]);

    plugin_sig!("@elapsed_ms", TypeInfo::UInt,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: []);

    plugin_sig!("@total_elapsed_ms", TypeInfo::UInt,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: []);

    plugin_sig!("@scope_message_count", TypeInfo::UInt,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: []);

    plugin_sig!("@scope_index", TypeInfo::UInt,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: []);

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_info_is_numeric() {
        assert!(TypeInfo::UInt.is_numeric());
        assert!(TypeInfo::Number.is_numeric());
        assert!(!TypeInfo::Bool.is_numeric());
        assert!(!TypeInfo::String.is_numeric());
        assert!(!TypeInfo::Time.is_numeric());
    }

    #[test]
    fn test_type_info_is_stringy() {
        assert!(TypeInfo::String.is_stringy());
        assert!(!TypeInfo::Bool.is_stringy());
        assert!(!TypeInfo::UInt.is_stringy());
        assert!(!TypeInfo::Number.is_stringy());
        assert!(!TypeInfo::Time.is_stringy());
    }

    #[test]
    fn test_type_info_is_temporal() {
        assert!(TypeInfo::Time.is_temporal());
        assert!(!TypeInfo::Bool.is_temporal());
        assert!(!TypeInfo::String.is_temporal());
        assert!(!TypeInfo::Number.is_temporal());
    }

    #[test]
    fn test_type_info_is_nullable() {
        assert!(TypeInfo::Any.is_nullable());
        assert!(!TypeInfo::Bool.is_nullable());
        assert!(!TypeInfo::String.is_nullable());
    }

    #[test]
    fn test_parse_type_name() {
        assert_eq!(TypeInfo::parse_type_name("bool"), Some(TypeInfo::Bool));
        assert_eq!(TypeInfo::parse_type_name("uint"), Some(TypeInfo::UInt));
        assert_eq!(TypeInfo::parse_type_name("number"), Some(TypeInfo::Number));
        assert_eq!(TypeInfo::parse_type_name("string"), Some(TypeInfo::String));
        assert_eq!(TypeInfo::parse_type_name("uuid"), Some(TypeInfo::String));
        assert_eq!(TypeInfo::parse_type_name("email"), Some(TypeInfo::String));
        assert_eq!(TypeInfo::parse_type_name("url"), Some(TypeInfo::String));
        assert_eq!(TypeInfo::parse_type_name("ip"), Some(TypeInfo::String));
        assert_eq!(TypeInfo::parse_type_name("time"), Some(TypeInfo::Time));
        assert_eq!(TypeInfo::parse_type_name("timestamp"), Some(TypeInfo::Time));
        assert_eq!(TypeInfo::parse_type_name("duration"), Some(TypeInfo::Time));
        assert_eq!(TypeInfo::parse_type_name("json"), Some(TypeInfo::Json));
        assert_eq!(TypeInfo::parse_type_name("yaml"), Some(TypeInfo::Yaml));
        assert_eq!(TypeInfo::parse_type_name("unknown"), None);
        assert_eq!(TypeInfo::parse_type_name(""), None);
    }

    #[test]
    fn test_base_type_is_self() {
        assert_eq!(TypeInfo::Bool.base_type(), TypeInfo::Bool);
        assert_eq!(TypeInfo::UInt.base_type(), TypeInfo::UInt);
        assert_eq!(TypeInfo::Number.base_type(), TypeInfo::Number);
        assert_eq!(TypeInfo::String.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Time.base_type(), TypeInfo::Time);
        assert_eq!(TypeInfo::Json.base_type(), TypeInfo::Json);
        assert_eq!(TypeInfo::Yaml.base_type(), TypeInfo::Yaml);
        assert_eq!(TypeInfo::Any.base_type(), TypeInfo::Any);
    }

    #[test]
    fn test_display_name() {
        assert_eq!(TypeInfo::Bool.display_name(), "bool");
        assert_eq!(TypeInfo::UInt.display_name(), "uint");
        assert_eq!(TypeInfo::Number.display_name(), "number");
        assert_eq!(TypeInfo::String.display_name(), "string");
        assert_eq!(TypeInfo::Time.display_name(), "time");
        assert_eq!(TypeInfo::Json.display_name(), "json");
        assert_eq!(TypeInfo::Yaml.display_name(), "yaml");
        assert_eq!(TypeInfo::Any.display_name(), "any");
    }

    #[test]
    fn test_supports_operator() {
        assert!(TypeInfo::Bool.supports_operator("==").0);
        assert!(!TypeInfo::Bool.supports_operator("<").0);
        assert!(!TypeInfo::Bool.supports_operator("contains").0);

        assert!(TypeInfo::UInt.supports_operator(">=").0);
        assert!(!TypeInfo::UInt.supports_operator("contains").0);

        assert!(TypeInfo::Number.supports_operator("<=").0);
        assert!(!TypeInfo::Number.supports_operator("startsWith").0);

        assert!(TypeInfo::Time.supports_operator(">=").0);
        assert!(TypeInfo::Time.supports_operator("<").0);
        assert!(!TypeInfo::Time.supports_operator("contains").0);
        assert!(!TypeInfo::Time.supports_operator("startsWith").0);

        assert!(TypeInfo::String.supports_operator("contains").0);
        assert!(TypeInfo::String.supports_operator("startsWith").0);
        assert!(TypeInfo::String.supports_operator("endsWith").0);
        assert!(TypeInfo::String.supports_operator("matches").0);
        assert!(!TypeInfo::String.supports_operator(">=").0);

        assert!(TypeInfo::Any.supports_operator("==").0);
        assert!(!TypeInfo::Any.supports_operator(">=").0);
    }

    #[test]
    fn test_typed_plugin_signature_valid_arg_count() {
        let sig = TypedPluginSignature {
            return_type: TypeInfo::Bool,
            arg_types: &[
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: true,
                    default: None,
                },
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: false,
                    default: Some("default"),
                },
            ],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
        };

        assert!(sig.valid_arg_count(1));
        assert!(sig.valid_arg_count(2));
        assert!(!sig.valid_arg_count(0));
        assert!(!sig.valid_arg_count(3));
    }

    #[test]
    fn test_typed_plugin_signature_count() {
        let sig = TypedPluginSignature {
            return_type: TypeInfo::Bool,
            arg_types: &[
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: true,
                    default: None,
                },
                ArgTypeInfo {
                    expected: TypeInfo::String,
                    required: false,
                    default: Some("default"),
                },
            ],
            purity: crate::PluginPurity::Pure,
            deterministic: true,
            idempotent: true,
            safe_for_rewrite: true,
        };

        assert_eq!(sig.arg_type(0), Some(TypeInfo::String));
        assert_eq!(sig.arg_type(1), Some(TypeInfo::String));
        assert_eq!(sig.arg_type(2), None);
    }

    #[test]
    fn test_typed_plugin_signatures_keys() {
        let sigs = typed_plugin_signatures();
        assert!(sigs.contains_key("@uuid"));
        assert!(sigs.contains_key("@email"));
        assert!(sigs.contains_key("@ip"));
        assert!(sigs.contains_key("@url"));
        assert!(sigs.contains_key("@len"));
        assert!(sigs.contains_key("@empty"));
        assert!(sigs.contains_key("@header"));
        assert!(sigs.contains_key("@trailer"));
        assert!(sigs.contains_key("@env"));
        assert!(sigs.contains_key("@elapsed_ms"));
    }
}
