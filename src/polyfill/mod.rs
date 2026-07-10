pub use cfg_runtime::*;

/// Re-export module for backward compatibility.
/// Items from `cfg_runtime` are also available at `crate::polyfill` directly.
pub mod runtime {
    pub use cfg_runtime::*;
}
