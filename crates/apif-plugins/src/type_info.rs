//! Extended type system for plugin arguments and return values.
//!
//! Enables the optimizer to make smarter rewrites and the LSP to show
//! accurate type information in hover/completion.

use serde::{Deserialize, Serialize};

/// Extended type information for plugin return values and arguments.
/// More specific than `PluginReturnKind` — enables optimizer rewrites
/// and semantic validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeInfo {
    // ─── Scalar types ───
    /// Boolean: true / false
    Bool,
    /// Non-negative integer (0, 1, 2, ...). Used by @len, @scope_message_count, etc.
    UInt,
    /// Any number (integer or float). Used by @elapsed_ms, etc.
    Number,
    /// String value. Used by @header, @trailer, @env, etc.
    String,

    // ─── Structured types ───
    /// JSON object/array. Used in REQUEST, RESPONSE, ERROR sections.
    Json,
    /// YAML document. Used in configuration sections (EXTRACT, OPTIONS).
    Yaml,

    // ─── Nullable variants ───
    /// Boolean or null (when lookup fails)
    BoolOrNull,
    /// String or null (when lookup fails)
    StringOrNull,
    /// JSON or null (when lookup fails)
    JsonOrNull,
    /// YAML or null (when lookup fails)
    YamlOrNull,

    // ─── Constrained strings ───
    /// String that matches UUID format
    Uuid,
    /// String that matches email format
    Email,
    /// String that matches URL format
    Url,
    /// String that matches IPv4/IPv6 format
    Ip,

    // ─── Wildcard ───
    /// JSON value of any type. Used by plugins that return the input as-is.
    Any,
}

impl TypeInfo {
    /// Check if this type can be used in a boolean context.
    pub fn is_truthy(&self) -> bool {
        matches!(self, TypeInfo::Bool | TypeInfo::BoolOrNull)
    }

    /// Check if this type can be compared with numbers.
    pub fn is_numeric(&self) -> bool {
        matches!(self, TypeInfo::UInt | TypeInfo::Number)
    }

    /// Check if this type can be compared with strings.
    pub fn is_stringy(&self) -> bool {
        matches!(
            self,
            TypeInfo::String
                | TypeInfo::StringOrNull
                | TypeInfo::Uuid
                | TypeInfo::Email
                | TypeInfo::Url
                | TypeInfo::Ip
        )
    }

    /// Check if this type can ever be null.
    pub fn is_nullable(&self) -> bool {
        matches!(
            self,
            TypeInfo::BoolOrNull
                | TypeInfo::StringOrNull
                | TypeInfo::JsonOrNull
                | TypeInfo::YamlOrNull
                | TypeInfo::Any
        )
    }

    /// Return the "base" type (strip nullability and constraints).
    pub fn base_type(&self) -> TypeInfo {
        match self {
            TypeInfo::BoolOrNull => TypeInfo::Bool,
            TypeInfo::StringOrNull => TypeInfo::String,
            TypeInfo::JsonOrNull => TypeInfo::Json,
            TypeInfo::YamlOrNull => TypeInfo::Yaml,
            TypeInfo::Uuid | TypeInfo::Email | TypeInfo::Url | TypeInfo::Ip => TypeInfo::String,
            other => *other,
        }
    }

    /// Human-readable display name for LSP hover / explain / inspect.
    pub fn display_name(&self) -> &'static str {
        match self {
            TypeInfo::Bool => "bool",
            TypeInfo::UInt => "non-negative integer",
            TypeInfo::Number => "number",
            TypeInfo::String => "string",
            TypeInfo::Json => "json",
            TypeInfo::Yaml => "yaml",
            TypeInfo::Any => "any",
            TypeInfo::BoolOrNull => "bool | null",
            TypeInfo::StringOrNull => "string | null",
            TypeInfo::JsonOrNull => "json | null",
            TypeInfo::YamlOrNull => "yaml | null",
            TypeInfo::Uuid => "uuid (string)",
            TypeInfo::Email => "email (string)",
            TypeInfo::Url => "url (string)",
            TypeInfo::Ip => "ip address (string)",
        }
    }

    /// Check if a comparison operator makes sense for this type.
    /// Returns `(true, None)` if the operator is valid,
    /// or `(false, Some(reason))` with a human-readable explanation.
    pub fn supports_operator(&self, op: &str) -> (bool, Option<&'static str>) {
        match op {
            "==" | "!=" => (true, None), // All types support equality
            ">" | "<" | ">=" | "<=" => {
                if self.is_numeric() {
                    (true, None)
                } else if matches!(self, TypeInfo::Bool | TypeInfo::BoolOrNull) {
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
                } else if self.is_numeric() {
                    (
                        false,
                        Some(
                            "numeric values do not support string operators — use == or != instead",
                        ),
                    )
                } else if matches!(self, TypeInfo::Bool | TypeInfo::BoolOrNull) {
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

/// Type information for a single plugin argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgTypeInfo {
    /// Expected type of this argument.
    pub expected: TypeInfo,
    /// Is this argument required? If false, it has a default value.
    pub required: bool,
    /// Default value expression (for optional args).
    pub default: Option<&'static str>,
}

/// Extended plugin signature with full type information.
#[derive(Debug, Clone)]
pub struct TypedPluginSignature {
    /// Return type of the plugin.
    pub return_type: TypeInfo,
    /// Type information for each argument.
    pub arg_types: &'static [ArgTypeInfo],
    /// Plugin purity (same as PluginSignature).
    pub purity: crate::PluginPurity,
    /// Is the plugin deterministic? (same as PluginSignature).
    pub deterministic: bool,
    /// Is the plugin idempotent? (same as PluginSignature).
    pub idempotent: bool,
    /// Is it safe for the optimizer to rewrite expressions using this plugin?
    pub safe_for_rewrite: bool,
}

impl TypedPluginSignature {
    /// Check if the plugin can be called with the given number of arguments.
    pub fn valid_arg_count(&self, count: usize) -> bool {
        let required = self.arg_types.iter().filter(|a| a.required).count();
        let total = self.arg_types.len();
        count >= required && count <= total
    }

    /// Get expected type for argument at given position.
    pub fn arg_type(&self, idx: usize) -> Option<TypeInfo> {
        self.arg_types.get(idx).map(|a| a.expected)
    }
}

/// Build typed signatures for all known plugins.
/// This is the single source of truth for plugin types.
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

    // ─── Validation plugins (return bool) ───
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

    // ─── Length / emptiness plugins ───
    plugin_sig!("@len", TypeInfo::UInt,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::Any, required: true, default: None }]);

    plugin_sig!("@empty", TypeInfo::Bool,
        purity: PluginPurity::Pure, deterministic: true, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::Any, required: true, default: None }]);

    // ─── Header / trailer plugins ───
    plugin_sig!("@header", TypeInfo::StringOrNull,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@has_header", TypeInfo::Bool,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@trailer", TypeInfo::StringOrNull,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: false,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    plugin_sig!("@has_trailer", TypeInfo::Bool,
        purity: PluginPurity::ContextDependent, deterministic: false, idempotent: true, rewrite: true,
        args: [ArgTypeInfo { expected: TypeInfo::String, required: true, default: None }]);

    // ─── Environment plugin ───
    plugin_sig!("@env", TypeInfo::StringOrNull,
    purity: PluginPurity::Impure, deterministic: false, idempotent: false, rewrite: false,
    args: [
        ArgTypeInfo { expected: TypeInfo::String, required: true, default: None },
        ArgTypeInfo { expected: TypeInfo::String, required: false, default: None },
    ]);

    // ─── Timing plugins ───
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
    fn test_type_info_is_truthy() {
        assert!(TypeInfo::Bool.is_truthy());
        assert!(TypeInfo::BoolOrNull.is_truthy());
        assert!(!TypeInfo::String.is_truthy());
        assert!(!TypeInfo::UInt.is_truthy());
    }

    #[test]
    fn test_type_info_is_numeric() {
        assert!(TypeInfo::UInt.is_numeric());
        assert!(TypeInfo::Number.is_numeric());
        assert!(!TypeInfo::String.is_numeric());
    }

    #[test]
    fn test_type_info_is_stringy() {
        assert!(TypeInfo::String.is_stringy());
        assert!(TypeInfo::StringOrNull.is_stringy());
        assert!(TypeInfo::Uuid.is_stringy());
        assert!(TypeInfo::Email.is_stringy());
        assert!(TypeInfo::Url.is_stringy());
        assert!(TypeInfo::Ip.is_stringy());
        assert!(!TypeInfo::Bool.is_stringy());
    }

    #[test]
    fn test_type_info_is_nullable() {
        assert!(TypeInfo::BoolOrNull.is_nullable());
        assert!(TypeInfo::StringOrNull.is_nullable());
        assert!(TypeInfo::JsonOrNull.is_nullable());
        assert!(TypeInfo::YamlOrNull.is_nullable());
        assert!(TypeInfo::Any.is_nullable());
        assert!(!TypeInfo::Bool.is_nullable());
    }

    #[test]
    fn test_type_info_base_type() {
        assert_eq!(TypeInfo::BoolOrNull.base_type(), TypeInfo::Bool);
        assert_eq!(TypeInfo::StringOrNull.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Uuid.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Email.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Url.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Ip.base_type(), TypeInfo::String);
        assert_eq!(TypeInfo::Bool.base_type(), TypeInfo::Bool);
    }

    #[test]
    fn test_type_info_display_name() {
        assert_eq!(TypeInfo::Bool.display_name(), "bool");
        assert_eq!(TypeInfo::UInt.display_name(), "non-negative integer");
        assert_eq!(TypeInfo::Number.display_name(), "number");
        assert_eq!(TypeInfo::String.display_name(), "string");
        assert_eq!(TypeInfo::Json.display_name(), "json");
        assert_eq!(TypeInfo::Yaml.display_name(), "yaml");
        assert_eq!(TypeInfo::Any.display_name(), "any");
        assert_eq!(TypeInfo::BoolOrNull.display_name(), "bool | null");
        assert_eq!(TypeInfo::Uuid.display_name(), "uuid (string)");
        assert_eq!(TypeInfo::Email.display_name(), "email (string)");
        assert_eq!(TypeInfo::Url.display_name(), "url (string)");
        assert_eq!(TypeInfo::Ip.display_name(), "ip address (string)");
    }

    #[test]
    fn test_type_info_supports_operator() {
        let (valid, _) = TypeInfo::Bool.supports_operator("==");
        assert!(valid);

        let (valid, reason) = TypeInfo::Bool.supports_operator("<");
        assert!(!valid);
        assert!(reason.is_some());

        let (valid, reason) = TypeInfo::String.supports_operator("<");
        assert!(!valid);
        assert!(reason.is_some());

        let (valid, _) = TypeInfo::String.supports_operator("contains");
        assert!(valid);

        let (valid, _) = TypeInfo::Number.supports_operator("contains");
        assert!(!valid);

        let (valid, _) = TypeInfo::Uuid.supports_operator("==");
        assert!(valid);

        let (valid, reason) = TypeInfo::Uuid.supports_operator(">");
        assert!(!valid);
        assert!(reason.is_some());

        let (valid, _reason) = TypeInfo::Uuid.supports_operator("contains");
        assert!(valid);

        let (valid, _) = TypeInfo::Any.supports_operator("==");
        assert!(valid);
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
    fn test_typed_plugin_signatures() {
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
