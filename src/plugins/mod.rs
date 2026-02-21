pub mod email;
pub mod external;
pub mod header;
pub mod ip;
pub mod len;
pub mod timestamp;
pub mod trailer;
pub mod url;
pub mod uuid;

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::assert::engine::AssertionResult;

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
        manager.load_external_plugins();
        manager
    }

    fn register_defaults(&mut self) {
        self.register(Arc::new(uuid::UuidPlugin));
        self.register(Arc::new(email::EmailPlugin));
        self.register(Arc::new(ip::IpPlugin));
        self.register(Arc::new(url::UrlPlugin));
        self.register(Arc::new(timestamp::TimestampPlugin));
        self.register(Arc::new(header::HeaderPlugin));
        self.register(Arc::new(trailer::TrailerPlugin));
        self.register(Arc::new(len::LenPlugin));
    }

    pub fn load_external_plugins(&mut self) {
        let home_dir = match dirs::home_dir() {
            Some(path) => path,
            None => {
                tracing::warn!("Could not determine home directory, skipping external plugins");
                return;
            }
        };

        let plugin_dir = home_dir.join(".grpctestify/plugins");
        if !plugin_dir.exists() {
            return;
        }

        tracing::info!("Loading plugins from {}", plugin_dir.display());

        let entries = match std::fs::read_dir(&plugin_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("Failed to read plugin directory: {}", e);
                return;
            }
        };

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("plugin.json");
                    if manifest_path.exists() {
                        self.load_plugin(&manifest_path, &path);
                    }
                }
            }
        }
    }

    fn load_plugin(&mut self, manifest_path: &std::path::Path, plugin_dir: &std::path::Path) {
        match std::fs::read_to_string(manifest_path) {
            Ok(content) => {
                match serde_json::from_str::<external::PluginManifest>(&content) {
                    Ok(manifest) => {
                        // Validate executable exists
                        let exec_path = plugin_dir.join(&manifest.executable);
                        if !exec_path.exists() {
                            tracing::error!(
                                "Plugin '{}' executable not found: {}",
                                manifest.name,
                                exec_path.display()
                            );
                            return;
                        }

                        // Iterate over all functions defined in the manifest
                        for func in manifest.functions {
                            let plugin_name = func.name.clone();
                            let plugin = Arc::new(external::ExternalPlugin::new(
                                func.name,
                                exec_path.clone(),
                                func.description,
                            ));

                            tracing::info!(
                                "Registering external plugin function '{}' from {}",
                                plugin_name,
                                manifest.name
                            );
                            self.register(plugin);
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to parse plugin manifest {}: {}",
                            manifest_path.display(),
                            e
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to read plugin manifest {}: {}",
                    manifest_path.display(),
                    e
                );
            }
        }
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
