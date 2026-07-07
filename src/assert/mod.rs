// Thin shim — all implementation lives in crates/apif-assert.
pub use apif_assert::{
    AssertionEngine, AssertionResult, AssertionTiming, JsonComparator, NoopPluginRegistry,
    PluginApi, PluginContext, PluginRegistry, PluginResult, get_json_diff,
};
pub use apif_assert::{comparator, diff, engine, operators, registry};
