use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    Ident(String),
    StringLit(String),
    NumberLit(String),
    Eq,
    Ne,
    Gte,
    Lte,
    Gt,
    Lt,
    Tilde,
    Colon,
    Comma,
    LParen,
    RParen,
    Dot,
    EOF,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    Eq(String),
    Ne(String),
    Gte(String),
    Lte(String),
    Gt(String),
    Lt(String),
    Like(String),
    Regex(String),
    In(Vec<String>),
    Between { min: String, max: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterExpr {
    pub column: String,
    pub op: FilterOp,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub source: String,
    pub filters: Vec<FilterExpr>,
    pub source_span: Span,
}

pub struct Lexer<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
    chars: Vec<char>,
    pos: usize,
}

impl Lexer<'_> {
    pub fn new(input: &str) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        self.pos += 1;
        ch
    }

    fn span(&self, start: usize) -> Span {
        Span::new(start, self.pos)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self, quote: char) -> String {
        let _start = self.pos;
        self.advance();
        let mut result = String::new();
        while let Some(c) = self.advance() {
            if c == quote {
                break;
            }
            if c == '\\' {
                if let Some(escaped) = self.advance() {
                    match escaped {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        'r' => result.push('\r'),
                        '\\' => result.push('\\'),
                        '"' => result.push('"'),
                        _ => result.push(escaped),
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();

        let start = self.pos;
        let ch = self.advance()?;

        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            ',' => TokenKind::Comma,
            '~' => TokenKind::Tilde,
            ':' => TokenKind::Colon,
            '.' => TokenKind::Dot,
            '=' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::Eq
            }
            '!' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::Ne
            }
            '>' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::Gte
            }
            '<' if self.peek() == Some('=') => {
                self.advance();
                TokenKind::Lte
            }
            '>' => TokenKind::Gt,
            '<' => TokenKind::Lt,
            '=' => TokenKind::Eq,
            '"' | '\'' => {
                let s = self.read_string(ch);
                TokenKind::StringLit(s)
            }
            c if c.is_alphanumeric() || c == '_' => {
                let mut ident = String::from(c);
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        self.advance();
                        ident.push(c);
                    } else {
                        break;
                    }
                }
                TokenKind::Ident(ident)
            }
            c if c.is_ascii_digit() => {
                let mut num = String::from(c);
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        self.advance();
                        num.push(c);
                    } else {
                        break;
                    }
                }
                TokenKind::NumberLit(num)
            }
            _ => TokenKind::Ident(String::from(ch)),
        };

        Some(Token {
            kind,
            span: self.span(start),
        })
    }
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Option<Token>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token();
        Self { lexer, current }
    }

    fn peek(&self) -> Option<&Token> {
        self.current.as_ref()
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.current.take();
        self.current = self.lexer.next_token();
        token
    }

    fn parse_value(&mut self) -> Result<String> {
        let token = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
        match token.kind {
            TokenKind::StringLit(s) => Ok(s),
            TokenKind::Ident(s) => Ok(s),
            TokenKind::NumberLit(n) => Ok(n),
            _ => Err(anyhow::anyhow!(
                "unexpected token {:?}, expected value",
                token.kind
            )),
        }
    }

    fn parse_filter(&mut self) -> Result<FilterExpr> {
        let span_start = self.lexer.pos;

        let column_token = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF in filter"))?;
        let column = match column_token.kind {
            TokenKind::Ident(s) => s,
            TokenKind::StringLit(s) => s,
            _ => {
                return Err(anyhow::anyhow!(
                    "unexpected token {:?}, expected column name",
                    column_token.kind
                ));
            }
        };

        let op_token = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF in filter"))?;

        let op = match op_token.kind {
            TokenKind::Eq => {
                let value = self.parse_value()?;
                if value.contains(',') {
                    let parts: Vec<String> =
                        value.split(',').map(|s| s.trim().to_string()).collect();
                    FilterOp::In(parts)
                } else {
                    FilterOp::Eq(value)
                }
            }
            TokenKind::Ne => {
                let value = self.parse_value()?;
                FilterOp::Ne(value)
            }
            TokenKind::Gte => {
                let value = self.parse_value()?;
                FilterOp::Gte(value)
            }
            TokenKind::Lte => {
                let value = self.parse_value()?;
                FilterOp::Lte(value)
            }
            TokenKind::Gt => {
                let value = self.parse_value()?;
                FilterOp::Gt(value)
            }
            TokenKind::Lt => {
                let value = self.parse_value()?;
                FilterOp::Lt(value)
            }
            TokenKind::Tilde => {
                let ident_token = self
                    .advance()
                    .ok_or_else(|| anyhow::anyhow!("expected glob or re after ~"))?;
                let ident = match ident_token.kind {
                    TokenKind::Ident(s) => s,
                    _ => return Err(anyhow::anyhow!("expected glob or re after ~")),
                };
                match ident.as_str() {
                    "glob" => {
                        let value = self.parse_value()?;
                        FilterOp::Like(value)
                    }
                    "re" => {
                        self.expect(TokenKind::Colon)?;
                        let value = self.parse_value()?;
                        FilterOp::Regex(value)
                    }
                    _ => {
                        return Err(anyhow::anyhow!("invalid operator ~{}", ident));
                    }
                }
            }
            _ => {
                return Err(anyhow::anyhow!("invalid operator {:?}", op_token.kind));
            }
        };

        Ok(FilterExpr {
            column,
            op,
            span: Span::new(span_start, self.lexer.pos),
        })
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Token> {
        let token = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?;
        if std::mem::discriminant(&token.kind) != std::mem::discriminant(&expected) {
            return Err(anyhow::anyhow!(
                "unexpected token {:?}, expected {:?}",
                token.kind,
                expected
            ));
        }
        Ok(token)
    }

    pub fn parse_query(&mut self) -> Result<Query> {
        let source_start = self.lexer.pos;

        let source_token = self
            .advance()
            .ok_or_else(|| anyhow::anyhow!("empty source name"))?;
        let source = match source_token.kind {
            TokenKind::Ident(s) => s,
            TokenKind::StringLit(s) => s,
            _ => return Err(anyhow::anyhow!("empty source name")),
        };

        let source_span = Span::new(source_start, source_start + source.len());

        let mut filters = Vec::new();

        while let Some(token) = self.peek() {
            if matches!(token.kind, TokenKind::Ident(_) | TokenKind::StringLit(_)) {
                let mut filter = self.parse_filter()?;
                // Check for IN operator: if Eq with value followed by comma
                if let FilterOp::Eq(first_val) = &filter.op {
                    let mut values = vec![first_val.clone()];
                    while matches!(
                        self.peek(),
                        Some(Token {
                            kind: TokenKind::Comma,
                            ..
                        })
                    ) {
                        self.advance(); // consume comma
                        if let Some(Token {
                            kind:
                                TokenKind::Ident(s) | TokenKind::StringLit(s) | TokenKind::NumberLit(s),
                            ..
                        }) = self.peek()
                        {
                            values.push(s.clone());
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if values.len() > 1 {
                        filter.op = FilterOp::In(values);
                    }
                }
                filters.push(filter);
            } else {
                break;
            }
        }

        Ok(Query {
            source,
            filters,
            source_span,
        })
    }
}

pub fn parse_query(input: &str) -> Result<Query> {
    let mut parser = Parser::new(input);
    parser.parse_query()
}

impl FilterExpr {
    pub fn matches(&self, row: &std::collections::HashMap<String, String>) -> bool {
        let value = match row.get(&self.column) {
            Some(v) => v,
            None => return false,
        };

        match &self.op {
            FilterOp::Eq(v) => value == v,
            FilterOp::Ne(v) => value != v,
            FilterOp::Gte(v) => value >= v,
            FilterOp::Lte(v) => value <= v,
            FilterOp::Gt(v) => value > v,
            FilterOp::Lt(v) => value < v,
            FilterOp::Like(pattern) => glob_match(pattern, value),
            FilterOp::Regex(pattern) => regex_match(pattern, value),
            FilterOp::In(vals) => vals.contains(value),
            FilterOp::Between { min, max } => value >= min && value <= max,
        }
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.replace('*', ".*").replace('?', ".");
    if let Ok(re) = regex::Regex::new(&format!("^{}$", pattern)) {
        re.is_match(value)
    } else {
        false
    }
}

fn regex_match(pattern: &str, value: &str) -> bool {
    if let Ok(re) = regex::Regex::new(pattern) {
        re.is_match(value)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_simple_query() {
        let result = parse_query("users status=active").unwrap();
        assert_eq!(result.source, "users");
        assert_eq!(result.filters.len(), 1);
        assert_eq!(result.filters[0].column, "status");
    }

    #[test]
    fn test_query_with_multiple_filters() {
        let result = parse_query("users status=active age>=18").unwrap();
        assert_eq!(result.source, "users");
        assert_eq!(result.filters.len(), 2);
    }

    #[test]
    fn test_query_with_like() {
        let result = parse_query(r#"users name~glob"*John*"#).unwrap();
        assert_eq!(result.source, "users");
        assert_eq!(result.filters.len(), 1);
    }

    #[test]
    fn test_query_with_regex() {
        let result = parse_query(r#"users msg~re:"error|warn""#).unwrap();
        assert_eq!(result.source, "users");
        assert_eq!(result.filters.len(), 1);
    }

    #[test]
    fn test_filter_matches() {
        let row: HashMap<String, String> = HashMap::from([
            ("status".into(), "active".into()),
            ("age".into(), "25".into()),
        ]);

        let query = parse_query("users status=active").unwrap();
        assert!(query.filters[0].matches(&row));

        let query2 = parse_query("users status=pending").unwrap();
        assert!(!query2.filters[0].matches(&row));
    }

    #[test]
    fn test_in_operator() {
        let row: HashMap<String, String> = HashMap::from([("status".into(), "active".into())]);

        let query = parse_query("users status=active,pending,waiting").unwrap();
        assert_eq!(query.filters.len(), 1);
        assert!(query.filters[0].matches(&row));

        let query2 = parse_query("users status=pending,waiting").unwrap();
        assert!(!query2.filters[0].matches(&row));
    }

    #[test]
    fn test_comparison_operators() {
        let row: HashMap<String, String> = HashMap::from([("age".into(), "25".into())]);

        let query = parse_query("users age>=18").unwrap();
        assert!(query.filters[0].matches(&row));

        let query2 = parse_query("users age>=30").unwrap();
        assert!(!query2.filters[0].matches(&row));

        let query3 = parse_query("users age<30").unwrap();
        assert!(query3.filters[0].matches(&row));

        let query4 = parse_query("users age<=25").unwrap();
        assert!(query4.filters[0].matches(&row));
    }

    #[test]
    fn test_multiple_filters() {
        let row: HashMap<String, String> = HashMap::from([
            ("status".into(), "active".into()),
            ("age".into(), "25".into()),
        ]);

        let query = parse_query("users status=active age>=18").unwrap();
        assert_eq!(query.filters.len(), 2);
        assert!(query.filters[0].matches(&row));
        assert!(query.filters[1].matches(&row));
    }
}
