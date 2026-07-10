//! Public tokenizer for GCTF assertion expressions.
//!
//! Pipeline: `text → tokenize_assertion() → Vec<Token>` where each `Token`
//! has a `Span { start, end }` with exact byte positions in the source.
//!
//! This is used by:
//! - **LSP semantic tokens** — highlight operators, keywords, plugins, regexes
//! - **Optimizer** — safe string-literal-aware rule matching (4.3)
//! - **Semantics** — type-checking operators against TypeInfo

use serde::{Deserialize, Serialize};

/// Byte range in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Token kinds produced by the assertion tokenizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    Ident(String),
    StringLit(String),
    NumberLit(String),
    Op(String),
    RegExpLit { pattern: String, flags: String },
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Comma,
    Bang,
    Pipe,
    Slash,
    Colon,
    VarDelim,
}

/// A token with its source position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Tokenize an assertion expression string into a list of tokens with
/// exact byte positions.
pub fn tokenize_assertion(source: &str) -> Vec<Token> {
    let mut out = Vec::with_capacity(source.len() / 2);
    let cs: Vec<char> = source.chars().collect();
    let mut i = 0;

    while i < cs.len() {
        match cs[i] {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '@' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::At, Span { start: s, end: i }));
            }
            '(' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::LParen, Span { start: s, end: i }));
            }
            ')' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::RParen, Span { start: s, end: i }));
            }
            '[' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::LBracket, Span { start: s, end: i }));
            }
            ']' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::RBracket, Span { start: s, end: i }));
            }
            '{' => {
                if i + 1 < cs.len() && cs[i + 1] == '{' {
                    let s = i;
                    i += 2;
                    out.push(Token::new(TokenKind::VarDelim, Span { start: s, end: i }));
                } else {
                    let s = i;
                    i += 1;
                    out.push(Token::new(TokenKind::LBrace, Span { start: s, end: i }));
                }
            }
            '}' => {
                if i + 1 < cs.len() && cs[i + 1] == '}' {
                    let s = i;
                    i += 2;
                    out.push(Token::new(TokenKind::VarDelim, Span { start: s, end: i }));
                } else {
                    let s = i;
                    i += 1;
                    out.push(Token::new(TokenKind::RBrace, Span { start: s, end: i }));
                }
            }
            '.' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::Dot, Span { start: s, end: i }));
            }
            ',' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::Comma, Span { start: s, end: i }));
            }
            '|' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::Pipe, Span { start: s, end: i }));
            }
            '!' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    let s = i;
                    i += 2;
                    out.push(Token::new(
                        TokenKind::Op("!=".into()),
                        Span { start: s, end: i },
                    ));
                } else {
                    let s = i;
                    i += 1;
                    out.push(Token::new(TokenKind::Bang, Span { start: s, end: i }));
                }
            }
            '=' if i + 1 < cs.len() && cs[i + 1] == '=' => {
                let s = i;
                i += 2;
                out.push(Token::new(
                    TokenKind::Op("==".into()),
                    Span { start: s, end: i },
                ));
            }
            '=' => i += 1,
            '>' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    let s = i;
                    i += 2;
                    out.push(Token::new(
                        TokenKind::Op(">=".into()),
                        Span { start: s, end: i },
                    ));
                } else {
                    let s = i;
                    i += 1;
                    out.push(Token::new(
                        TokenKind::Op(">".into()),
                        Span { start: s, end: i },
                    ));
                }
            }
            '<' => {
                if i + 1 < cs.len() && cs[i + 1] == '=' {
                    let s = i;
                    i += 2;
                    out.push(Token::new(
                        TokenKind::Op("<=".into()),
                        Span { start: s, end: i },
                    ));
                } else {
                    let s = i;
                    i += 1;
                    out.push(Token::new(
                        TokenKind::Op("<".into()),
                        Span { start: s, end: i },
                    ));
                }
            }
            '-' => {
                let s = i;
                i += 1;
                out.push(Token::new(
                    TokenKind::Op("-".into()),
                    Span { start: s, end: i },
                ));
            }
            '"' => {
                let s = i;
                i += 1;
                let mut v = String::new();
                while i < cs.len() && cs[i] != '"' {
                    if cs[i] == '\\' && i + 1 < cs.len() {
                        i += 1;
                        v.push(cs[i]);
                    } else {
                        v.push(cs[i]);
                    }
                    i += 1;
                }
                if i < cs.len() {
                    i += 1;
                }
                out.push(Token::new(
                    TokenKind::StringLit(v),
                    Span { start: s, end: i },
                ));
            }
            c if c.is_ascii_digit() => {
                let s = i;
                let mut v = String::new();
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                    v.push(cs[i]);
                    i += 1;
                }
                out.push(Token::new(
                    TokenKind::NumberLit(v),
                    Span { start: s, end: i },
                ));
            }
            c if c.is_alphabetic() || c == '_' => {
                let s = i;
                let mut v = String::new();
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    v.push(cs[i]);
                    i += 1;
                }
                let kind = match v.as_str() {
                    "contains" | "matches" | "startsWith" | "endsWith" | "startswith"
                    | "endswith" => TokenKind::Op(v),
                    _ => TokenKind::Ident(v),
                };
                out.push(Token::new(kind, Span { start: s, end: i }));
            }
            ':' => {
                let s = i;
                i += 1;
                out.push(Token::new(TokenKind::Colon, Span { start: s, end: i }));
            }
            '$' => {
                let s = i;
                i += 1;
                let mut v = String::from('$');
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                    v.push(cs[i]);
                    i += 1;
                }
                out.push(Token::new(TokenKind::Ident(v), Span { start: s, end: i }));
            }
            '/' => {
                let s = i;
                let mut j = i + 1;
                let mut is_regex = true;
                let mut pattern = String::new();
                let mut escaped = false;
                let mut in_cc = false;
                while j < cs.len() {
                    if escaped {
                        pattern.push(cs[j]);
                        escaped = false;
                        j += 1;
                    } else if cs[j] == '\\' {
                        pattern.push(cs[j]);
                        escaped = true;
                        j += 1;
                    } else if cs[j] == '[' {
                        in_cc = true;
                        pattern.push(cs[j]);
                        j += 1;
                    } else if cs[j] == ']' {
                        in_cc = false;
                        pattern.push(cs[j]);
                        j += 1;
                    } else if cs[j] == '/' && !in_cc {
                        j += 1;
                        let mut flags = String::new();
                        while j < cs.len()
                            && cs[j].is_ascii_alphabetic()
                            && "gimsuy".contains(cs[j])
                        {
                            flags.push(cs[j]);
                            j += 1;
                        }
                        if j < cs.len()
                            && (cs[j].is_alphanumeric() || cs[j] == '_')
                            && !flags.is_empty()
                        {
                            is_regex = false;
                        }
                        break;
                    } else if cs[j] == '\n' || cs[j] == '\r' || cs[j] == ' ' || cs[j] == '\t' {
                        is_regex = false;
                        break;
                    } else {
                        pattern.push(cs[j]);
                        j += 1;
                    }
                }
                if is_regex && !pattern.is_empty() && j > i + 1 {
                    out.push(Token::new(
                        TokenKind::RegExpLit {
                            pattern,
                            flags: String::new(),
                        },
                        Span { start: s, end: j },
                    ));
                    i = j;
                } else {
                    i += 1;
                    out.push(Token::new(TokenKind::Slash, Span { start: s, end: i }));
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

/// Collect all identifier tokens.
pub fn collect_identifiers(tokens: &[Token]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Ident(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

/// Collect all operator tokens.
pub fn collect_operators(tokens: &[Token]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Op(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

/// Collect plugin call names with spans.
pub fn collect_plugin_calls(tokens: &[Token]) -> Vec<(&str, Span)> {
    let mut result = Vec::with_capacity(tokens.len() / 4);
    for [at, ident] in tokens.array_windows::<2>() {
        if let TokenKind::At = at.kind
            && let TokenKind::Ident(name) = &ident.kind
        {
            result.push((name.as_str(), at.span));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize_assertion(".id == 123");
        assert!(!tokens.is_empty());
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Op(s) if s == "=="))
        );
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::NumberLit(s) if s == "123"))
        );
    }

    #[test]
    fn test_tokenize_plugin_call() {
        let tokens = tokenize_assertion("@len(.items) == 0");
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::At)));
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "len"))
        );
    }

    #[test]
    fn test_tokenize_regex() {
        let tokens = tokenize_assertion("@regex(.x, /hello/i) == true");
        assert!(tokens.iter().any(
            |t| matches!(&t.kind, TokenKind::RegExpLit { pattern, .. } if pattern == "hello")
        ));
    }

    #[test]
    fn test_tokenize_string_literal() {
        let tokens = tokenize_assertion(".name == \"hello world\"");
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::StringLit(s) if s == "hello world"))
        );
    }

    #[test]
    fn test_spans_correct() {
        let tokens = tokenize_assertion(".x == 0");
        // "==" should be at position 3
        if let Some(t) = tokens
            .iter()
            .find(|t| matches!(&t.kind, TokenKind::Op(s) if s == "=="))
        {
            assert_eq!(t.span.start, 3);
            assert_eq!(t.span.end, 5);
        } else {
            panic!("No == token found");
        }
    }
}
