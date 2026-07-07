pub mod assertions;
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

// Re-export AST modules from apif-ast so crate::ast::* paths work
pub use apif_ast::{ast, assertion_ast, gctf_tokenizer, tokenizer};

pub use apif_ast::{
    AssertionExpr, BinaryOp, Expr, FileMeta, GctfAttribute, GctfDocument, InlineOptions, Literal,
    Section, SectionContent, SectionHeader, SectionType, Span, Token, TokenKind,
    assertion_to_string, parse_assertion, remove_redundant_parens,
    tokenize_assertion, tokenize_gctf, tokenize_inline_options, tokenize_kv_line,
};
pub use builder::GctfDocumentBuilder;
pub use content_parser::{build_section, parse_inline_options, parse_section_content};
pub use core::{ParseDiagnostics, parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics, serialize_gctf};
pub use document_splitter::split_sections_by_boundary;
pub use error_recovery::{ErrorRecoveryResult, parse_content_with_recovery, parse_with_recovery};
pub use ternary::{process_extract_value, ternary_to_jq};
pub use ternary_ast::{ExtractValue, ExtractVar};
pub use validator::{
    ErrorSeverity, ValidationError, validate_document, validate_document_diagnostics,
    BENCH_ASSERT_MODE_VALUES, BENCH_CACHE_VALUES, BENCH_DURATION_KEYS, BENCH_DURATION_STOP_VALUES,
    BENCH_LOAD_SCHEDULE_VALUES, BENCH_MODE_VALUES, BENCH_NUMERIC_KEYS, allowed_values_message,
    canonical_bench_key, is_allowed_value, supported_bench_keys,
};
