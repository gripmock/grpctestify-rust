//! Assertion plugins — built-in functions for gRPC test assertions.
//!
//! Plugins extend the assertion engine with custom validation logic.
//! Each plugin implements the [`Plugin`] trait and is registered with [`PluginManager`].
//!
//! # Type System
//!
//! Plugin signatures include full type information (`TypeInfo`, `ArgTypeInfo`)
//! that is consumed by:
//! - **Optimizer**: type-aware rewrites (e.g., `@len(.x) >= 0 → true`)
//! - **LSP**: hover information, completion, signature help
//! - **Semantics**: type-checking assertion expressions
//! - **Explain/Inspect**: human-readable type information
//!
//! # Available Plugins
//!
//! | Plugin | Purpose | Returns |
//! |--------|---------|---------|
//! | `@uuid` | Validate UUID format | bool |
//! | `@email` | Validate email format | bool |
//! | `@ip` | Validate IP address | bool |
//! | `@url` | Validate URL format | bool |
//! | `@timestamp` | Validate Unix timestamp | bool |
//! | `@regex` | Regex matching | bool |
//! | `@len` / `@empty` | Length/emptiness checks | non-negative integer / bool |
//! | `@header` / `@has_header` | HTTP header extraction/checks | string|null / bool |
//! | `@trailer` / `@has_trailer` | gRPC trailer extraction/checks | string|null / bool |
//! | `@env` | Environment variable (with optional default) | string|null |
//! | `@elapsed_ms` / `@total_elapsed_ms` | Timing assertions | non-negative integer |
//! | `@scope.message_count` / `@scope.index` | Streaming scope info | non-negative integer |

pub use crate::type_info::{ArgTypeInfo, TypeInfo, TypedPluginSignature};

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use apif_assert::registry;

// Re-export shared types from assert module (single source of truth)
pub use registry::{AssertionTiming, PluginContext, PluginResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPurity {
    Pure,
    ContextDependent,
    Impure,
}

#[derive(Debug, Clone, Copy)]
pub struct PluginSignature {
    /// Extended return type — the single source of truth for return type information.
    pub return_type: TypeInfo,
    /// Type information for each argument (for LSP signature help).
    pub arg_types: &'static [ArgTypeInfo],
    pub purity: PluginPurity,
    pub deterministic: bool,
    pub idempotent: bool,
    pub safe_for_rewrite: bool,
    /// Human-readable argument names for signature display.
    pub arg_names: &'static [&'static str],
    /// Canonical plugin name to use instead (e.g., `Some("is_uuid")` for deprecated `"uuid"`).
    /// `None` means the plugin name is current and not deprecated.
    pub replacement: Option<&'static str>,
}

impl Default for PluginSignature {
    fn default() -> Self {
        Self {
            return_type: TypeInfo::Any,
            arg_types: &[],
            purity: PluginPurity::Impure,
            deterministic: false,
            idempotent: false,
            safe_for_rewrite: false,
            arg_names: &[],
            replacement: None,
        }
    }
}

/// Trait for all plugins
pub trait Plugin: Send + Sync {
    /// unique name of the plugin (e.g., "uuid", "len")
    fn name(&self) -> &str;
    /// Description of what the plugin does
    fn description(&self) -> &str;
    /// Execute the plugin logic
    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult>;

    /// Static plugin signature used by optimizer/LSP.
    fn signature(&self) -> PluginSignature {
        PluginSignature::default()
    }
}

pub fn normalize_plugin_name(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.strip_prefix('@').unwrap_or(trimmed)
}

pub fn extract_plugin_call_name(expr: &str) -> Option<String> {
    let e = expr.trim();
    if !e.starts_with('@') || !e.ends_with(')') {
        return None;
    }

    let open = e.find('(')?;
    if open <= 1 {
        return None;
    }

    Some(e[1..open].trim().to_string())
}

pub fn plugin_signature_map() -> HashMap<String, PluginSignature> {
    PluginManager::new()
        .list()
        .into_iter()
        .map(|plugin| (plugin.name().to_string(), plugin.signature()))
        .collect()
}

/// Cached plugin signatures — single source of truth for all modules.
pub static PLUGIN_SIGNATURES: LazyLock<HashMap<String, PluginSignature>> =
    LazyLock::new(plugin_signature_map);

/// Manager to register and retrieve plugins
pub struct PluginManager {
    plugins: RwLock<HashMap<String, Arc<dyn Plugin>>>,
}

impl PluginManager {
    pub fn new() -> Self {
        let mut manager = Self {
            plugins: RwLock::new(HashMap::new()),
        };
        manager.register_defaults();
        manager
    }

    fn register_defaults(&mut self) {
        let plugin_pairs: [(&str, &str); 6] = [
            ("uuid", "is_uuid"),
            ("email", "is_email"),
            ("ip", "is_ip"),
            ("url", "is_url"),
            ("timestamp", "is_timestamp"),
            ("empty", "is_empty"),
        ];

        // Register original plugins under their current names
        self.register(Arc::new(crate::uuid::UuidPlugin));
        self.register(Arc::new(crate::email::EmailPlugin));
        self.register(Arc::new(crate::ip::IpPlugin));
        self.register(Arc::new(crate::url::UrlPlugin));
        self.register(Arc::new(crate::timestamp::TimestampPlugin));
        self.register(Arc::new(crate::empty::EmptyPlugin));

        // Register canonical (is_*) and deprecated (old) names
        for &(old_name, new_name) in &plugin_pairs {
            // Get the original plugin to wrap
            let inner = self.get(old_name).expect("plugin must be registered");

            // Canonical name (is_*) — replacement=None means not deprecated
            self.register_with_name(
                new_name,
                Arc::new(RenamedPlugin {
                    inner: inner.clone(),
                    new_name,
                    replacement: None,
                }),
            );

            // Old name — deprecated, replacement=Some(new_name)
            self.register_with_name(
                old_name,
                Arc::new(RenamedPlugin {
                    inner,
                    new_name: old_name,
                    replacement: Some(new_name),
                }),
            );
        }

        // Non-deprecated plugins
        self.register(Arc::new(crate::header_extract::HeaderExtractPlugin));
        self.register(Arc::new(crate::header_extract::HasHeaderPlugin));
        self.register(Arc::new(crate::trailer_extract::TrailerExtractPlugin));
        self.register(Arc::new(crate::trailer_extract::HasTrailerPlugin));
        self.register(Arc::new(crate::len::LenPlugin));
        self.register(Arc::new(crate::env::EnvPlugin));
        self.register(Arc::new(crate::regex::RegexPlugin));
        self.register(Arc::new(crate::timing::ElapsedMsPlugin));
        self.register(Arc::new(crate::timing::TotalElapsedMsPlugin));
        self.register(Arc::new(crate::timing::ScopeMessageCountPlugin));
        self.register(Arc::new(crate::timing::ScopeIndexPlugin));

        // Register canonical scope plugin names (@scope.message_count, @scope.index)
        // and deprecated old names (@scope_message_count, @scope_index)
        let scope_pairs: [(&str, &str); 2] = [
            ("scope_message_count", "scope.message_count"),
            ("scope_index", "scope.index"),
        ];
        for &(old_name, new_name) in &scope_pairs {
            let inner = self.get(new_name).expect("scope plugin must be registered");
            // Old name — deprecated, use new name instead
            self.register_with_name(
                old_name,
                Arc::new(RenamedPlugin {
                    inner,
                    new_name: old_name,
                    replacement: Some(new_name),
                }),
            );
        }

        // New plugins
        self.register(Arc::new(crate::has_value::HasValuePlugin));
        self.register(Arc::new(crate::is_base64::IsBase64Plugin));
        self.register(Arc::new(crate::is_json::IsJsonPlugin));

        // Type methods (@type.method syntax)
        crate::type_methods::register_all(self);
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins
            .write()
            .expect("lock poisoned")
            .insert(plugin.name().to_string(), plugin);
    }

    pub fn register_with_name(&mut self, name: &str, plugin: Arc<dyn Plugin>) {
        self.plugins
            .write()
            .expect("lock poisoned")
            .insert(name.to_string(), plugin);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        let normalized = normalize_plugin_name(name);
        self.plugins
            .read()
            .expect("lock poisoned")
            .get(normalized)
            .cloned()
    }

    pub fn list(&self) -> Vec<Arc<dyn Plugin>> {
        self.plugins
            .read()
            .expect("lock poisoned")
            .values()
            .cloned()
            .collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper that renames a plugin and optionally marks it deprecated.
/// Used for `is_*` aliases and old deprecated names.
struct RenamedPlugin {
    inner: Arc<dyn Plugin>,
    new_name: &'static str,
    /// `None` = not deprecated. `Some(x)` = deprecated, use `x` instead.
    replacement: Option<&'static str>,
}

impl Plugin for RenamedPlugin {
    fn name(&self) -> &str {
        self.new_name
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn execute(&self, args: &[Value], context: &PluginContext) -> Result<PluginResult> {
        self.inner.execute(args, context)
    }
    fn signature(&self) -> PluginSignature {
        let mut sig = self.inner.signature();
        sig.replacement = self.replacement;
        sig
    }
}

/// Wrapper to convert Arc<dyn Plugin> to Arc<dyn PluginApi>
struct PluginApiWrapper(Arc<dyn Plugin>);

impl apif_assert::registry::PluginApi for PluginApiWrapper {
    fn execute(
        &self,
        args: &[serde_json::Value],
        context: &apif_assert::registry::PluginContext,
    ) -> anyhow::Result<apif_assert::registry::PluginResult> {
        let ctx = apif_assert::PluginContext {
            response: context.response,
            headers: context.headers,
            trailers: context.trailers,
            timing: context.timing,
        };
        let result = self.0.execute(args, &ctx)?;
        match result {
            apif_assert::PluginResult::Assertion(a) => {
                Ok(apif_assert::registry::PluginResult::Assertion(a))
            }
            apif_assert::PluginResult::Value(v) => {
                Ok(apif_assert::registry::PluginResult::Value(v))
            }
        }
    }
}

impl apif_assert::registry::PluginRegistry for PluginManager {
    fn get_plugin(
        &self,
        name: &str,
    ) -> Option<std::sync::Arc<dyn apif_assert::registry::PluginApi>> {
        let normalized = normalize_plugin_name(name);
        self.plugins
            .read()
            .expect("lock poisoned")
            .get(normalized)
            .map(|p| {
                let p: Arc<dyn apif_assert::registry::PluginApi> =
                    Arc::new(PluginApiWrapper(p.clone()));
                p
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manager_new() {
        let manager = PluginManager::new();
        // PluginManager registers defaults on creation
        let plugins = manager.plugins.read().unwrap();
        // Should have default plugins registered
        assert!(!plugins.is_empty());
    }

    #[test]
    fn test_plugin_manager_register() {
        let mut manager = PluginManager::new();
        // Test registration
        let plugin = Arc::new(crate::uuid::UuidPlugin);
        manager.register(plugin);
        let plugins = manager.plugins.read().unwrap();
        assert!(plugins.contains_key("uuid"));
    }

    #[test]
    fn test_plugin_manager_get() {
        let manager = PluginManager::new();
        // Test retrieval of registered plugin
        let plugin = manager.get("uuid");
        assert!(plugin.is_some());
        assert_eq!(plugin.unwrap().name(), "uuid");
    }

    #[test]
    fn test_plugin_manager_get_accepts_at_prefix() {
        let manager = PluginManager::new();
        let plugin = manager.get("@uuid");
        assert!(plugin.is_some());
        assert_eq!(plugin.unwrap().name(), "uuid");
    }

    #[test]
    fn test_plugin_manager_list() {
        let manager = PluginManager::new();
        let plugins = manager.list();
        // Should have at least the default plugins
        assert!(plugins.len() >= 8); // uuid, email, ip, url, timestamp, header, trailer, len
    }

    #[test]
    fn test_plugin_manager_execute_plugin() {
        let manager = PluginManager::new();
        // Test execution with real plugin (uuid)
        let plugin = manager.get("uuid").unwrap();
        let context = PluginContext::new(&Value::Null);
        let result = plugin.execute(&[Value::String("test".to_string())], &context);
        // UUID plugin should return a value
        assert!(result.is_ok());
    }

    #[test]
    fn test_plugin_manager_has_header_registered() {
        let manager = PluginManager::new();
        let plugin = manager.get("has_header");
        assert!(plugin.is_some(), "has_header plugin should be registered");
        assert_eq!(plugin.unwrap().name(), "has_header");
    }

    #[test]
    fn test_plugin_manager_empty_registered() {
        let manager = PluginManager::new();
        let plugin = manager.get("empty");
        assert!(plugin.is_some(), "empty plugin should be registered");
        assert_eq!(plugin.unwrap().name(), "empty");
    }

    #[test]
    fn test_plugin_manager_has_trailer_registered() {
        let manager = PluginManager::new();
        let plugin = manager.get("has_trailer");
        assert!(plugin.is_some(), "has_trailer plugin should be registered");
        assert_eq!(plugin.unwrap().name(), "has_trailer");
    }

    #[test]
    fn test_signature_metadata_empty() {
        let manager = PluginManager::new();
        let signature = manager.get("empty").unwrap().signature();
        assert_eq!(signature.return_type, TypeInfo::Bool);
        assert_eq!(signature.purity, PluginPurity::Pure);
        assert!(signature.deterministic);
        assert!(signature.idempotent);
        assert!(signature.safe_for_rewrite);
    }

    #[test]
    fn test_signature_metadata_env() {
        let manager = PluginManager::new();
        let signature = manager.get("env").unwrap().signature();
        assert_eq!(signature.return_type, TypeInfo::String);
        assert_eq!(signature.purity, PluginPurity::Impure);
        assert!(!signature.deterministic);
        assert!(!signature.idempotent);
        assert!(!signature.safe_for_rewrite);
    }

    #[test]
    fn test_all_plugin_contracts() {
        let manager = PluginManager::new();
        let plugins = manager.list();
        assert!(!plugins.is_empty(), "should have registered plugins");

        for plugin in &plugins {
            let name = plugin.name();
            assert!(!name.is_empty(), "plugin name must not be empty");

            let desc = plugin.description();
            assert!(
                !desc.is_empty(),
                "plugin {} description must not be empty",
                name
            );

            let sig = plugin.signature();

            // Verify RenamedPlugin sets replacement correctly
            if name.starts_with("is_") {
                // Canonical name — replacement must be None
                assert!(
                    sig.replacement.is_none(),
                    "canonical plugin {} should not have replacement",
                    name
                );
            }

            // Verify deprecated names point to canonical
            let canonical = format!("is_{}", name);
            if manager.get(&canonical).is_some() && name != canonical {
                assert_eq!(
                    sig.replacement,
                    Some(canonical.as_str()),
                    "deprecated plugin {} should have replacement pointing to {}",
                    name,
                    canonical
                );
            }
        }
    }

    #[test]
    fn test_plugin_api_wrapper() {
        use apif_assert::registry::PluginRegistry;

        let manager = PluginManager::new();
        let api = manager
            .get_plugin("uuid")
            .expect("uuid should be registered");

        let result = api
            .execute(
                &[serde_json::Value::String(
                    "550e8400-e29b-41d4-a716-446655440000".into(),
                )],
                &apif_assert::registry::PluginContext {
                    response: &serde_json::Value::Null,
                    headers: None,
                    trailers: None,
                    timing: None,
                },
            )
            .unwrap();
        match result {
            apif_assert::registry::PluginResult::Assertion(
                apif_assert::engine::AssertionResult::Pass,
            ) => {}
            _ => panic!("Expected Pass assertion result"),
        }
    }

    #[test]
    fn test_plugin_manager_get_plugin_via_registry() {
        use apif_assert::registry::PluginRegistry;

        let manager = PluginManager::new();

        // get_plugin with @ prefix
        let plugin = manager.get_plugin("@uuid");
        assert!(plugin.is_some(), "should find @uuid");

        // get_plugin without @ prefix
        let plugin = manager.get_plugin("uuid");
        assert!(plugin.is_some(), "should find uuid");
    }

    #[test]
    fn test_plugin_manager_get_plugin_returns_none_for_unknown() {
        use apif_assert::registry::PluginRegistry;

        let manager = PluginManager::new();
        assert!(manager.get_plugin("nonexistent_plugin").is_none());
    }

    #[test]
    fn test_renamed_plugin_description() {
        let manager = PluginManager::new();

        // Both old and new names should have the same description
        let is_uuid = manager.get("is_uuid").unwrap();
        let uuid = manager.get("uuid").unwrap();
        assert_eq!(is_uuid.description(), uuid.description());
    }

    #[test]
    fn test_plugin_manager_default() {
        let manager = PluginManager::default();
        let plugins = manager.list();
        assert!(!plugins.is_empty());
    }

    #[test]
    fn test_plugin_signature_map_has_entries() {
        let map = plugin_signature_map();
        assert!(!map.is_empty(), "plugin signature map should not be empty");

        // Verify known plugins have entries
        assert!(map.contains_key("uuid"), "uuid should be in signature map");
        assert!(
            map.contains_key("is_uuid"),
            "is_uuid should be in signature map"
        );
        assert!(
            map.contains_key("email"),
            "email should be in signature map"
        );
        assert!(map.contains_key("len"), "len should be in signature map");
    }

    #[test]
    fn test_extract_plugin_call_name_various() {
        // Valid calls
        assert_eq!(extract_plugin_call_name("@uuid(.x)").unwrap(), "uuid");
        assert_eq!(
            extract_plugin_call_name("@url.scheme(.x)").unwrap(),
            "url.scheme"
        );
        assert_eq!(
            extract_plugin_call_name("@is_email(.x)").unwrap(),
            "is_email"
        );

        // Invalid calls
        assert!(
            extract_plugin_call_name("uuid(.x)").is_none(),
            "missing @ prefix"
        );
        assert!(
            extract_plugin_call_name("@uuid").is_none(),
            "missing parens"
        );
        assert!(extract_plugin_call_name("@()").is_none(), "empty name");
        assert!(extract_plugin_call_name("").is_none(), "empty string");
    }

    #[test]
    fn test_normalize_plugin_name() {
        assert_eq!(normalize_plugin_name("@uuid"), "uuid");
        assert_eq!(normalize_plugin_name("uuid"), "uuid");
        assert_eq!(normalize_plugin_name(" @uuid "), "uuid");
        assert_eq!(normalize_plugin_name(" "), "");
    }
}
