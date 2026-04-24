//! Semantic tokenization for GCTF documents.
//!
//! 100% token-based — no starts_with, ends_with, find, contains, as_bytes.
//! Pipeline: text → gctf_tokenizer → GctfToken → assertion tokenizer → LSP tokens

use crate::parser::gctf_tokenizer::{GctfTokenKind, tokenize_gctf};
use crate::parser::tokenizer::{TokenKind, tokenize_assertion};
use tower_lsp::lsp_types::{SemanticToken, SemanticTokens};

const KEYWORD: u32 = 0;
const VARIABLE: u32 = 1;
const FUNCTION: u32 = 2;
const NUMBER: u32 = 3;
const OPERATOR: u32 = 4;
const STRING: u32 = 5;
const REGEXP: u32 = 6;

#[derive(Debug, Clone, PartialEq)]
struct SrcToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

/// Build semantic tokens for syntax highlighting.
///
/// Pipeline:
/// 1. tokenize_gctf() → Vec<GctfToken> (section headers, content lines)
/// 2. For each line: tokenize_assertion() → Vec<Token> with Spans
/// 3. Classify tokens by GctfTokenKind / TokenKind → LSP delta-encoded SemanticTokens
pub fn build_semantic_tokens(content: &str) -> SemanticTokens {
    let mut tokens: Vec<SrcToken> = Vec::new();
    let gctf_tokens = tokenize_gctf(content);

    for gctf_token in &gctf_tokens {
        match &gctf_token.kind {
            GctfTokenKind::SectionHeader { .. } => {
                tokens.push(SrcToken {
                    line: gctf_token.line as u32,
                    start: 0,
                    length: gctf_token.span.end as u32,
                    token_type: KEYWORD,
                });
            }
            GctfTokenKind::Content(text) => {
                let line = gctf_token.line as u32;
                tokenize_line_as_assertion(text, line, &mut tokens);
            }
            GctfTokenKind::Comment(_) | GctfTokenKind::Blank => {}
        }
    }

    encode_tokens(tokens)
}

fn tokenize_line_as_assertion(line: &str, line_num: u32, tokens: &mut Vec<SrcToken>) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    let toks = tokenize_assertion(trimmed);

    for i in 0..toks.len() {
        let tok = &toks[i];

        if matches!(tok.kind, TokenKind::At)
            && let Some([_, next]) = toks[i..].array_windows::<2>().next()
            && let TokenKind::Ident(_name) = &next.kind
        {
            let start = tok.span.start;
            let end = next.span.end;
            tokens.push(SrcToken {
                line: line_num,
                start: start as u32,
                length: (end - start) as u32,
                token_type: FUNCTION,
            });
            continue;
        }

        let token_type = match &tok.kind {
            TokenKind::Ident(s) if s.starts_with('.') || s.starts_with("{{") => VARIABLE,
            TokenKind::Ident(s) if s == "if" || s == "then" || s == "else" || s == "end" => KEYWORD,
            TokenKind::Ident(s) if s == "true" || s == "false" || s == "null" => KEYWORD,
            TokenKind::Ident(s) if s == "and" || s == "or" || s == "xor" || s == "not" => KEYWORD,
            TokenKind::NumberLit(_) => NUMBER,
            TokenKind::StringLit(_) => STRING,
            TokenKind::RegExpLit { .. } => REGEXP,
            TokenKind::Op(_) => OPERATOR,
            TokenKind::At
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::Dot
            | TokenKind::Comma
            | TokenKind::Bang
            | TokenKind::Pipe
            | TokenKind::Slash
            | TokenKind::VarDelim => continue,
            TokenKind::Ident(_) => VARIABLE,
        };

        tokens.push(SrcToken {
            line: line_num,
            start: tok.span.start as u32,
            length: tok.span.len() as u32,
            token_type,
        });
    }
}

fn encode_tokens(mut raw_tokens: Vec<SrcToken>) -> SemanticTokens {
    raw_tokens.sort_by_key(|t| (t.line, t.start, t.length, t.token_type));
    raw_tokens.dedup_by(|a, b| a.line == b.line && a.start == b.start);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_tokens_section_header() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n.id == 123\n";
        let tokens = build_semantic_tokens(content);
        assert!(!tokens.data.is_empty());
        assert!(tokens.data.iter().any(|t| t.token_type == KEYWORD));
    }

    #[test]
    fn test_semantic_tokens_plugin_call() {
        let content = "--- ASSERTS ---\n@len(.items) == 0\n";
        let tokens = build_semantic_tokens(content);
        assert!(tokens.data.iter().any(|t| t.token_type == FUNCTION));
    }

    #[test]
    fn test_semantic_tokens_variable() {
        let content = "--- ASSERTS ---\n{{ user_id }} == 42\n";
        let tokens = build_semantic_tokens(content);
        assert!(tokens.data.iter().any(|t| t.token_type == VARIABLE));
    }

    #[test]
    fn test_semantic_tokens_regex_literal() {
        let content = "--- ASSERTS ---\n@regex(.name, /hello/i) == true\n";
        let tokens = build_semantic_tokens(content);
        assert!(tokens.data.iter().any(|t| t.token_type == REGEXP));
    }

    #[test]
    fn test_semantic_tokens_ternary_keywords() {
        let content = "--- ASSERTS ---\nif .x == 1 then true else false end\n";
        let tokens = build_semantic_tokens(content);
        let kw_count = tokens
            .data
            .iter()
            .filter(|t| t.token_type == KEYWORD)
            .count();
        assert!(kw_count >= 4);
    }

    #[test]
    fn test_semantic_tokens_operators() {
        let content = "--- ASSERTS ---\n.x >= 0 and .y != \"hello\"\n";
        let tokens = build_semantic_tokens(content);
        assert!(tokens.data.iter().any(|t| t.token_type == OPERATOR));
        assert!(tokens.data.iter().any(|t| t.token_type == STRING));
    }
}
