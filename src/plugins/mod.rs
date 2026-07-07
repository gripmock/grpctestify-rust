// Thin shim — all implementation lives in crates/apif-plugins.
pub use apif_plugins::{
    email, empty, env, header_extract, ip, len, macros, regex, timestamp, timing,
    trailer_extract, type_info, url, uuid,
};
pub use apif_plugins::{
    ArgTypeInfo, PLUGIN_SIGNATURES, Plugin, PluginManager, PluginPurity, PluginSignature,
    TypeInfo, TypedPluginSignature, extract_plugin_call_name, normalize_plugin_name,
    plugin_signature_map,
};
// Plugin context types re-exported for backward compatibility
pub use apif_plugins::{AssertionTiming, PluginContext, PluginResult};
