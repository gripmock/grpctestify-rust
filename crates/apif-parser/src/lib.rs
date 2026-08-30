pub mod assertions;
pub mod builder;
pub mod content_parser;
pub mod core;
pub mod deprecations;
pub mod document_splitter;
pub mod error_recovery;
pub mod json_mod;
pub mod json_stream_parser;
pub mod ternary;
pub mod ternary_ast;
pub mod validator;

pub use apif_ast::{assertion_ast, ast, gctf_tokenizer, tokenizer};

pub use apif_ast::{
    AssertionExpr, BinaryOp, DEPRECATED_KEBAB_CASE_KEYS, Expr, FileMeta, GctfAttribute,
    GctfDocument, InlineOptions, Literal, OrderedStringMap, Section, SectionContent, SectionHeader,
    SectionSpan, SectionType, Span, Token, TokenKind, assertion_to_string, canonical_key_spelling,
    line_start_byte_offsets, parse_assertion, remove_redundant_parens, tokenize_assertion,
    tokenize_gctf, tokenize_inline_options, tokenize_kv_line,
};
pub use builder::GctfDocumentBuilder;
pub use content_parser::{
    build_section, parse_inline_options, parse_section_content, register_extra_inline_option_keys,
};
pub use core::{
    ParseDiagnostics, parse_gctf, parse_gctf_from_str, parse_gctf_with_diagnostics, serialize_gctf,
    serialize_gctf_as_written,
};
pub use deprecations::detect_deprecations;
pub use document_splitter::split_sections_by_boundary;
pub use error_recovery::{ErrorRecoveryResult, parse_content_with_recovery, parse_with_recovery};
pub use ternary::{process_extract_value, ternary_to_jq};
pub use ternary_ast::{ExtractValue, ExtractVar};
pub use validator::{
    BENCH_ASSERT_MODE_VALUES, BENCH_CACHE_VALUES, BENCH_DURATION_KEYS, BENCH_DURATION_STOP_VALUES,
    BENCH_LOAD_SCHEDULE_VALUES, BENCH_MODE_VALUES, BENCH_NUMERIC_KEYS, ErrorSeverity,
    ValidationError, allowed_values_message, canonical_bench_key, is_allowed_value,
    supported_bench_keys, validate_document, validate_document_chain,
    validate_document_chain_diagnostics, validate_document_diagnostics,
};

pub struct MetaListProblem {
    pub key: String,
    pub message: String,
}

pub fn meta_list_problem(error: &str) -> Option<MetaListProblem> {
    let key = error.split(':').next()?.trim();
    if !matches!(key, "tags" | "links") {
        return None;
    }
    if !error.contains("invalid type: string") || !error.contains("expected a sequence") {
        return None;
    }
    let start = error.find('"')? + 1;
    let rest = &error[start..];
    let end = rest.rfind('"')?;
    let written = &rest[..end];
    let items: Vec<&str> = written
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(MetaListProblem {
        key: key.to_string(),
        message: format!(
            "META {key} is a list, not a line — write `{key}: [{}]`, or one `- {}` per line",
            items.join(", "),
            items[0]
        ),
    })
}

#[cfg(test)]
mod meta_list_problem_tests {
    use super::meta_list_problem;

    #[test]
    fn a_list_written_as_a_line_is_named_with_its_rewrite() {
        let said = meta_list_problem(
            "tags: invalid type: string \"smoke, billing\", expected a sequence at line 1 column 7",
        )
        .expect("a list problem");
        assert_eq!(said.key, "tags");
        assert_eq!(
            said.message,
            "META tags is a list, not a line — write `tags: [smoke, billing]`, or one `- smoke` per line"
        );
    }

    #[test]
    fn links_are_the_other_list() {
        let said = meta_list_problem(
            "links: invalid type: string \"https://a\", expected a sequence at line 2 column 8",
        )
        .expect("a list problem");
        assert_eq!(
            said.message,
            "META links is a list, not a line — write `links: [https://a]`, or one `- https://a` per line"
        );
    }

    #[test]
    fn other_meta_errors_are_left_alone() {
        for said in [
            "unknown field `tag`, expected one of `name`, `summary` at line 1 column 1",
            "owner: invalid type: map, expected a string at line 1 column 8",
            "tags: invalid type: string \"\", expected a sequence at line 1 column 7",
        ] {
            assert!(meta_list_problem(said).is_none(), "{said}");
        }
    }
}
