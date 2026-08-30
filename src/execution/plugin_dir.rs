pub fn build_plugin_manager() -> crate::plugins::PluginManager {
    let mut manager = crate::plugins::PluginManager::new();
    for plugin in crate::plugins::rhai_plugin::load_all_configured_plugins() {
        manager.register(plugin);
    }
    manager
}
