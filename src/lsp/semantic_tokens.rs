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
            GctfTokenKind::Comment(_) | GctfTokenKind::Blank | GctfTokenKind::AttributeBlock(_) => {
            }
        }
    }

    encode_tokens(tokens)
}

/// UTF-16 column of character offset `char_off` within `line`.
fn char_off_to_utf16(line: &str, char_off: usize) -> u32 {
    line.chars()
        .take(char_off)
        .map(|c| c.len_utf16() as u32)
        .sum()
}

/// Push a token whose span is relative to the trimmed assertion string.
///
/// `tokenize_assertion` runs on the leading-whitespace-trimmed line and reports
/// *character* offsets into that trimmed string, but LSP semantic tokens use
/// UTF-16 code-unit columns measured from the start of the *original* line. Add
/// back the indentation (`indent`, in characters) and convert character offsets
/// to UTF-16 so tokens land correctly on indented lines and lines containing
/// astral (surrogate-pair) characters.
fn push_span(
    tokens: &mut Vec<SrcToken>,
    line: &str,
    indent: usize,
    line_num: u32,
    start_c: usize,
    end_c: usize,
    token_type: u32,
) {
    let start = char_off_to_utf16(line, indent + start_c);
    let end = char_off_to_utf16(line, indent + end_c);
    tokens.push(SrcToken {
        line: line_num,
        start,
        length: end.saturating_sub(start),
        token_type,
    });
}

fn tokenize_line_as_assertion(line: &str, line_num: u32, tokens: &mut Vec<SrcToken>) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    // Number of leading-whitespace characters stripped by the trim above.
    let indent = line.chars().count() - line.trim_start().chars().count();

    let toks = tokenize_assertion(trimmed);

    for i in 0..toks.len() {
        let tok = &toks[i];

        if matches!(tok.kind, TokenKind::At)
            && let Some([_, next]) = toks[i..].array_windows::<2>().next()
            && let TokenKind::Ident(_name) = &next.kind
        {
            let start = tok.span.start;
            let mut end = next.span.end;
            let mut j = i + 2;
            // Consume .method for @type.method syntax
            while j + 1 < toks.len()
                && matches!(toks[j].kind, TokenKind::Dot)
                && matches!(toks[j + 1].kind, TokenKind::Ident(_))
            {
                end = toks[j + 1].span.end;
                j += 2;
            }
            push_span(tokens, line, indent, line_num, start, end, FUNCTION);
            continue;
        }

        let token_type = match &tok.kind {
            TokenKind::Ident(s)
                if s.starts_with('.') || s.starts_with("{{") || s.starts_with('$') =>
            {
                VARIABLE
            }
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
            | TokenKind::Colon
            | TokenKind::VarDelim => continue,
            TokenKind::Ident(_) => VARIABLE,
        };

        push_span(
            tokens,
            line,
            indent,
            line_num,
            tok.span.start,
            tok.span.end,
            token_type,
        );
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

    /// Decode the delta-encoded tokens back to absolute (line, start, length).
    fn absolute(tokens: &SemanticTokens) -> Vec<(u32, u32, u32, u32)> {
        let mut out = Vec::new();
        let mut line = 0u32;
        let mut start = 0u32;
        for t in &tokens.data {
            if t.delta_line == 0 {
                start += t.delta_start;
            } else {
                line += t.delta_line;
                start = t.delta_start;
            }
            out.push((line, start, t.length, t.token_type));
        }
        out
    }

    #[test]
    fn test_semantic_tokens_indented_line_keeps_column() {
        // The number `123` sits at column 8 because of the 2-space indent.
        // Tokenizing the trimmed line dropped the indent, mislocating it at 6.
        let content = "--- REQUEST ---\n  \"id\": 123\n";
        let toks = absolute(&build_semantic_tokens(content));
        let number = toks
            .iter()
            .find(|(line, _, _, tt)| *line == 1 && *tt == NUMBER)
            .expect("number token on line 1");
        assert_eq!(number.1, 8, "indent must be preserved in the column");
    }

    #[test]
    fn test_semantic_tokens_astral_utf16_column() {
        // The emoji key is a single `char` but two UTF-16 code units, so `42`
        // sits at char offset 6 but UTF-16 column 7. Emitting the raw char
        // offset mislocates the highlight by one on every following token.
        let content = "--- REQUEST ---\n{\"😀\": 42}\n";
        let toks = absolute(&build_semantic_tokens(content));
        let number = toks
            .iter()
            .find(|(line, _, _, tt)| *line == 1 && *tt == NUMBER)
            .expect("number token on line 1");
        assert_eq!(
            number.1, 7,
            "column must be a UTF-16 offset (emoji is 2 code units)"
        );
        assert_eq!(number.2, 2, "length of `42` is 2 UTF-16 units");
    }
}
