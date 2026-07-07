// Thin shim — all implementation lives in crates/apif-parser.
// Paths like `crate::parser::ast::GctfDocument` still work.

pub use apif_parser::{
    AssertionExpr, BinaryOp, ErrorSeverity, Expr, ExtractValue, ExtractVar, FileMeta,
    GctfAttribute, GctfDocument, GctfDocumentBuilder, InlineOptions, Literal, ParseDiagnostics,
    Section, SectionContent, SectionHeader, SectionType, Span, Token, TokenKind, ValidationError,
    assertion_to_string, build_section, parse_assertion, parse_content_with_recovery,
    parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics, parse_inline_options,
    parse_section_content, parse_with_recovery, process_extract_value, remove_redundant_parens,
    serialize_gctf, split_sections_by_boundary, ternary_to_jq, tokenize_assertion, tokenize_gctf,
    tokenize_inline_options, tokenize_kv_line, validate_document, validate_document_diagnostics,
    ErrorRecoveryResult,
};

// Re-export sub-modules for paths like `crate::parser::ast::*`
pub use apif_parser::{ast, assertion_ast, gctf_tokenizer, tokenizer};

// query_ast backward compat — resolves to `crate::parser::query_ast::*`
pub mod query_ast {
    pub use crate::parser::{parse_query, FilterExpr};
}
pub use apif_query::{parse_query, FilterExpr};

// Validator items
pub use apif_parser::validator;

// Other modules
pub use apif_parser::{
    assertions, builder, content_parser, core, document_splitter, error_recovery, json_mod,
    json_stream_parser, ternary, ternary_ast,
};
