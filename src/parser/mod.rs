// .gctf file parser with AST (Abstract Syntax Tree)
// This module provides robust parsing of .gctf files into an AST

pub mod ast;
pub mod core;
pub mod error_recovery;
pub mod json_mod;
pub mod ternary;
pub mod ternary_ast;
pub mod validator;

pub use ast::GctfDocument;
pub use core::{ParseDiagnostics, parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics};
pub use error_recovery::{ErrorRecoveryResult, parse_content_with_recovery, parse_with_recovery};
pub use ternary::{process_extract_value, ternary_to_jq};
pub use ternary_ast::{ExtractValue, ExtractVar, TernaryExpr};
pub use validator::{
    ErrorSeverity, ValidationError, validate_document, validate_document_diagnostics,
};
