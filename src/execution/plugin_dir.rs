// Assertion-plugin registry shared by the execution-side plugin registries.
//
// `response_handler.rs`/`assertion_handler.rs`/`runner.rs` each hold their
// own `LazyLock<Arc<dyn PluginRegistry>>` (one `PluginManager` per file,
// first-populated on first use); this module is where each of those builds
// its `PluginManager` from.

/// A default `PluginManager` (built-ins registered) plus every `.rhai`
/// plugin found under the configured convention directories — the
/// user-global `~/.grpctestify/plugins` and the project-local
/// `./.grpctestify/plugins` (see
/// [`apif_plugins::rhai_plugin::load_all_configured_plugins`] for the
/// precedence when a name is defined in both).
pub fn build_plugin_manager() -> crate::plugins::PluginManager {
    let mut manager = crate::plugins::PluginManager::new();
    for plugin in crate::plugins::rhai_plugin::load_all_configured_plugins() {
        manager.register(plugin);
    }
    manager
}
