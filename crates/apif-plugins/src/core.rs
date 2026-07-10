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
//! | `@scope_message_count` / `@scope_index` | Streaming scope info | non-negative integer |

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
    /// Used by optimizer, LSP hover, and semantics.
    pub return_type: TypeInfo,
    /// Type information for each argument (for LSP signature help).
    pub arg_types: &'static [ArgTypeInfo],
    pub purity: PluginPurity,
    pub deterministic: bool,
    pub idempotent: bool,
    pub safe_for_rewrite: bool,
    /// Human-readable argument names for signature display.
    pub arg_names: &'static [&'static str],
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
        self.register(Arc::new(crate::uuid::UuidPlugin));
        self.register(Arc::new(crate::email::EmailPlugin));
        self.register(Arc::new(crate::empty::EmptyPlugin));
        self.register(Arc::new(crate::ip::IpPlugin));
        self.register(Arc::new(crate::url::UrlPlugin));
        self.register(Arc::new(crate::timestamp::TimestampPlugin));
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
}
