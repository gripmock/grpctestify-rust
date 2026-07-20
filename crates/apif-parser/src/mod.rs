//! # Parser Module
//!
//! Pipeline: `text → tokenizer → parser → AST → semantic analysis → execution`
//!
//! Parses `.gctf` test files into an AST with support for:
//! - JSON5 with comments, trailing commas, unquoted keys
//! - Multi-document files via document chain (iter_chain)
//! - Error recovery parsing
//! - Ternary expressions in EXTRACT sections
//! - Full assertion AST with tokenization and span tracking

pub use apif_query::{parse_query, FilterExpr};
/// Re-export for backward compatibility — resolves to `crate::query_ast::*`
pub mod query_ast {
    pub use super::{parse_query, FilterExpr};
}

pub(crate) mod assertions;
pub mod builder;
pub mod content_parser;
pub mod core;
pub mod document_splitter;
pub mod error_recovery;
pub mod json_mod;
pub mod json_stream_parser;
pub mod ternary;
pub mod ternary_ast;
pub mod validator;

pub use apif_ast::{ast, assertion_ast, gctf_tokenizer, tokenizer};

pub use assertion_ast::{
    AssertionExpr, BinaryOp, Expr, Literal, assertion_to_string, parse_assertion,
    remove_redundant_parens,
};
pub use content_parser::{build_section, parse_inline_options, parse_section_content};
pub use tokenizer::{
    Span, Token, TokenKind, collect_identifiers, collect_operators, collect_plugin_calls,
    tokenize_assertion,
};

pub use gctf_tokenizer::{
    GctfToken, GctfTokenKind, tokenize_extract_line, tokenize_gctf, tokenize_inline_options,
    tokenize_kv_line,
};

pub use document_splitter::split_sections_by_boundary;

pub use ast::GctfDocument;
pub use builder::GctfDocumentBuilder;
pub use core::{ParseDiagnostics, parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics, serialize_gctf};
pub use error_recovery::{ErrorRecoveryResult, parse_content_with_recovery, parse_with_recovery};
pub use ternary::{process_extract_value, ternary_to_jq};
pub use ternary_ast::{ExtractValue, ExtractVar};
pub use validator::{
    ErrorSeverity, ValidationError, validate_document, validate_document_chain,
    validate_document_chain_diagnostics, validate_document_diagnostics,
};
