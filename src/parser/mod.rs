// .gctf file parser with AST (Abstract Syntax Tree)
// This module provides robust parsing of .gctf files into an AST

pub mod ast;
pub mod core;
pub mod json_mod;
pub mod validator;

pub use ast::GctfDocument;
pub use core::{parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics, ParseDiagnostics};
pub use validator::{
    validate_document, validate_document_diagnostics, ErrorSeverity, ValidationError,
};
