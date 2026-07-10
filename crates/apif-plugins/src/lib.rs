pub mod core;
pub mod email;
pub mod empty;
pub mod env;
pub mod header_extract;
pub mod ip;
pub mod len;
pub mod macros;
pub mod regex;
pub mod timestamp;
pub mod timing;
pub mod trailer_extract;
pub mod type_info;
pub mod url;
pub mod uuid;

pub use type_info::{ArgTypeInfo, TypeInfo, TypedPluginSignature};

// Re-export all pub items from core module
pub use core::{
    PLUGIN_SIGNATURES, Plugin, PluginManager, PluginPurity, PluginSignature,
    extract_plugin_call_name, normalize_plugin_name, plugin_signature_map,
};

// Re-export plugin context types from apif-assert (used by individual plugins)
pub use apif_assert::{AssertionTiming, PluginContext, PluginResult};
