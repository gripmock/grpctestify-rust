//! Semantic tokenization for GCTF documents.
//!
//! Provides syntax highlighting tokens for the LSP server using a hybrid approach:
//! AST-based for section headers, regex-based for inline content.

use crate::parser;
use tower_lsp::lsp_types::{SemanticToken, SemanticTokens};

/// Token type indices (must match the order in `initialize`).
const KEYWORD: u32 = 0;
const VARIABLE: u32 = 1;
const FUNCTION: u32 = 2;
const NUMBER: u32 = 3;
const OPERATOR: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RawToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

/// Build semantic tokens for syntax highlighting.
///
/// Uses a hybrid approach: AST for section headers, regex for inline content.
/// TODO: Full AST-based tokenization when parser tracks all token types.
pub fn build_semantic_tokens(content: &str) -> SemanticTokens {
    let section_header_re = regex::Regex::new(r"^---\s*[A-Z_]+(?:\s+.+)?\s*---$").ok();
    let jq_keyword_re = regex::Regex::new(r"\b(if|then|else|end|select|map|reduce|foreach|def|import|include|module|as|label|break)\b").ok();
    let variable_re = regex::Regex::new(r"\{\{[^}]+\}\}").ok();
    let plugin_re = regex::Regex::new(r"@[A-Za-z_][A-Za-z0-9_]*").ok();
    let number_re = regex::Regex::new(r"\b\d+(?:\.\d+)?\b").ok();
    let operator_re = regex::Regex::new(
        r"==|!=|<=|>=|\bcontains\b|\bmatches\b|\bstartsWith\b|\bendsWith\b|[<>+\-*/%|]",
    )
    .ok();

    let mut raw_tokens: Vec<RawToken> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let tokenize_line = |line: &str, line_num: u32, include_jq_keywords: bool| -> Vec<RawToken> {
        let mut line_tokens = Vec::new();
        if let Some(re) = &variable_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: VARIABLE,
                });
            }
        }
        if let Some(re) = &plugin_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: FUNCTION,
                });
            }
        }
        if let Some(re) = &number_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: NUMBER,
                });
            }
        }
        if let Some(re) = &operator_re {
            for m in re.find_iter(line) {
                line_tokens.push(RawToken {
                    line: line_num,
                    start: m.start() as u32,
                    length: (m.end() - m.start()) as u32,
                    token_type: OPERATOR,
                });
            }
        }
        if include_jq_keywords {
            #[allow(clippy::collapsible_if)]
            if let Some(re) = &jq_keyword_re {
                for m in re.find_iter(line) {
                    line_tokens.push(RawToken {
                        line: line_num,
                        start: m.start() as u32,
                        length: (m.end() - m.start()) as u32,
                        token_type: KEYWORD,
                    });
                }
            }
        }
        line_tokens
    };

    if let Ok(doc) = parser::parse_gctf_from_str(content, "temp.gctf") {
        if doc.sections.is_empty() {
            return fallback_tokenize(&lines, &section_header_re, &tokenize_line);
        }
        for section in &doc.sections {
            if section.start_line < lines.len() {
                let header_line = lines[section.start_line];
                if section_header_re
                    .as_ref()
                    .is_some_and(|re| re.is_match(header_line.trim()))
                {
                    let start = header_line.find("---").unwrap_or(0) as u32;
                    raw_tokens.push(RawToken {
                        line: section.start_line as u32,
                        start,
                        length: header_line.trim().len() as u32,
                        token_type: KEYWORD,
                    });
                }
            }
            for (idx, section_line) in section.raw_content.lines().enumerate() {
                let line_num = (section.start_line + idx + 1) as u32;
                let include_jq_keywords = section.section_type == parser::ast::SectionType::Extract;
                raw_tokens.extend(tokenize_line(section_line, line_num, include_jq_keywords));
            }
        }
    } else {
        return fallback_tokenize(&lines, &section_header_re, &tokenize_line);
    }

    encode_tokens(raw_tokens)
}

fn fallback_tokenize(
    lines: &[&str],
    section_header_re: &Option<regex::Regex>,
    tokenize_line: &impl Fn(&str, u32, bool) -> Vec<RawToken>,
) -> SemanticTokens {
    let mut raw_tokens = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;
        if section_header_re
            .as_ref()
            .is_some_and(|re| re.is_match(line.trim()))
        {
            raw_tokens.push(RawToken {
                line: line_num,
                start: line.find("---").unwrap_or(0) as u32,
                length: line.trim().len() as u32,
                token_type: KEYWORD,
            });
        }
        raw_tokens.extend(tokenize_line(line, line_num, true));
    }
    encode_tokens(raw_tokens)
}

fn encode_tokens(mut raw_tokens: Vec<RawToken>) -> SemanticTokens {
    raw_tokens.sort_by_key(|t| (t.line, t.start, t.length, t.token_type));
    raw_tokens.dedup();

    let mut encoded = Vec::with_capacity(raw_tokens.len());
    let mut last_line: u32 = 0;
    let mut last_start: u32 = 0;

    for t in raw_tokens {
        let delta_line = t.line.saturating_sub(last_line);
        let delta_start = if delta_line == 0 {
            t.start.saturating_sub(last_start)
        } else {
            t.start
        };
        encoded.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: 0,
        });
        last_line = t.line;
        last_start = t.start;
    }

    SemanticTokens {
        result_id: None,
        data: encoded,
    }
}
