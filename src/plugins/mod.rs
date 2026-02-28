pub mod email;
pub mod empty;
pub mod env;
pub mod header_extract;
pub mod ip;
pub mod len;
pub mod regex;
pub mod timestamp;
pub mod trailer_extract;
pub mod url;
pub mod uuid;

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::assert::engine::AssertionResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginReturnKind {
    Boolean,
    Number,
    String,
    Value,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPurity {
    Pure,
    ContextDependent,
    Impure,
}

#[derive(Debug, Clone, Copy)]
pub struct PluginSignature {
    pub return_kind: PluginReturnKind,
    pub purity: PluginPurity,
    pub deterministic: bool,
    pub idempotent: bool,
    pub safe_for_rewrite: bool,
    pub arg_names: &'static [&'static str],
}

impl Default for PluginSignature {
    fn default() -> Self {
        Self {
            return_kind: PluginReturnKind::Unknown,
            purity: PluginPurity::Impure,
            deterministic: false,
            idempotent: false,
            safe_for_rewrite: false,
            arg_names: &[],
        }
    }
}

/// Context passed to plugins during execution
pub struct PluginContext<'a> {
    pub response: &'a Value,
    pub headers: Option<&'a HashMap<String, String>>,
    pub trailers: Option<&'a HashMap<String, String>>,
}

/// Result of a plugin execution
#[derive(Debug, Clone)]
pub enum PluginResult {
    /// The plugin performed an assertion (pass/fail)
    Assertion(AssertionResult),
    /// The plugin computed a value to be used in further expressions
    Value(Value),
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
        self.register(Arc::new(uuid::UuidPlugin));
        self.register(Arc::new(email::EmailPlugin));
        self.register(Arc::new(empty::EmptyPlugin));
        self.register(Arc::new(ip::IpPlugin));
        self.register(Arc::new(url::UrlPlugin));
        self.register(Arc::new(timestamp::TimestampPlugin));
        self.register(Arc::new(header_extract::HeaderExtractPlugin));
        self.register(Arc::new(header_extract::HasHeaderPlugin));
        self.register(Arc::new(trailer_extract::TrailerExtractPlugin));
        self.register(Arc::new(trailer_extract::HasTrailerPlugin));
        self.register(Arc::new(len::LenPlugin));
        self.register(Arc::new(env::EnvPlugin));
        self.register(Arc::new(regex::RegexPlugin));
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins
            .write()
            .unwrap()
            .insert(plugin.name().to_string(), plugin);
    }

    pub fn register_with_name(&mut self, name: &str, plugin: Arc<dyn Plugin>) {
        self.plugins
            .write()
            .unwrap()
            .insert(name.to_string(), plugin);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.read().unwrap().get(name).cloned()
    }

    pub fn list(&self) -> Vec<Arc<dyn Plugin>> {
        self.plugins.read().unwrap().values().cloned().collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
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
        let plugin = Arc::new(uuid::UuidPlugin);
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
        let context = PluginContext {
            response: &Value::Null,
            headers: None,
            trailers: None,
        };
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
        assert_eq!(signature.return_kind, PluginReturnKind::Boolean);
        assert_eq!(signature.purity, PluginPurity::Pure);
        assert!(signature.deterministic);
        assert!(signature.idempotent);
        assert!(signature.safe_for_rewrite);
    }

    #[test]
    fn test_signature_metadata_env() {
        let manager = PluginManager::new();
        let signature = manager.get("@env").unwrap().signature();
        assert_eq!(signature.return_kind, PluginReturnKind::String);
        assert_eq!(signature.purity, PluginPurity::ContextDependent);
        assert!(!signature.deterministic);
        assert!(!signature.idempotent);
        assert!(!signature.safe_for_rewrite);
    }
}
