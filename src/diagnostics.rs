// Diagnostic utilities for better error reporting
// Foundation for LSP diagnostics

pub mod builder;
pub mod types;

pub use builder::{DiagnosticBuilder, GctfDiagnostics};
pub use types::*;
