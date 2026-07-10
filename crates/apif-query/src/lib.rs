use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
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
            FilterOp::Gte(v) => compare_values(value, ">=", v),
            FilterOp::Lte(v) => compare_values(value, "<=", v),
            FilterOp::Gt(v) => compare_values(value, ">", v),
            FilterOp::Lt(v) => compare_values(value, "<", v),
            FilterOp::Like(pattern) => like_match(pattern, value),
            FilterOp::Regex(pattern) => regex_match(pattern, value),
            FilterOp::In(vals) => vals.contains(value),
            FilterOp::Between { min, max } => {
                compare_values(value, ">=", min) && compare_values(value, "<=", max)
            }
        }
    }
}

thread_local! {
    static REGEX_CACHE: RefCell<std::collections::HashMap<String, std::result::Result<Rc<Regex>, String>>> =
        RefCell::new(std::collections::HashMap::new());
}

fn cached_regex(pattern: &str) -> std::result::Result<Rc<Regex>, String> {
    if let Some(cached) = REGEX_CACHE.with(|cache| cache.borrow().get(pattern).cloned()) {
        return cached;
    }
    let compiled = Regex::new(pattern).map(Rc::new).map_err(|e| e.to_string());
    REGEX_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(pattern.to_string(), compiled.clone());
    });
    compiled
}

fn like_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return value == pattern;
    }

    let has_start_star = pattern.starts_with('*');
    let has_end_star = pattern.ends_with('*');

    if has_start_star && has_end_star {
        // *literal* — contains check
        let mid = &pattern[1..pattern.len() - 1];
        if !mid.contains('*') && !mid.contains('?') {
            return value.contains(mid);
        }
    } else if has_start_star {
        // *literal — ends_with check
        let suffix = &pattern[1..];
        if !suffix.contains('*') && !suffix.contains('?') {
            return value.ends_with(suffix);
        }
    } else if has_end_star {
        // literal* — starts_with check
        let prefix = &pattern[..pattern.len() - 1];
        if !prefix.contains('*') && !prefix.contains('?') {
            return value.starts_with(prefix);
        }
    }

    // body has wildcards inside (e.g. "te*t") — needs regex
    let re_pat = pattern.replace('*', ".*").replace('?', ".");
    match cached_regex(&format!("^(?:{})$", re_pat)) {
        Ok(re) => re.is_match(value),
        Err(_) => false,
    }
}

pub fn glob_match(pattern: &str, value: &str) -> bool {
    like_match(pattern, value)
}

pub fn regex_match(pattern: &str, value: &str) -> bool {
    match cached_regex(pattern) {
        Ok(re) => re.is_match(value),
        Err(_) => false,
    }
}

pub fn compare_values(left: &str, op: &str, right: &str) -> bool {
    if let (Ok(l), Ok(r)) = (left.parse::<f64>(), right.parse::<f64>()) {
        return match op {
            ">=" => l >= r,
            "<=" => l <= r,
            ">" => l > r,
            "<" => l < r,
            _ => false,
        };
    }
    match op {
        ">=" => left >= right,
        "<=" => left <= right,
        ">" => left > right,
        "<" => left < right,
        _ => false,
    }
}

impl FilterOp {
    fn selectivity_rank(&self) -> u8 {
        match self {
            FilterOp::Eq(_) => 0,
            FilterOp::Ne(_) => 1,
            FilterOp::Gte(_)
            | FilterOp::Lte(_)
            | FilterOp::Gt(_)
            | FilterOp::Lt(_)
            | FilterOp::Between { .. } => 2,
            FilterOp::In(_) => 3,
            FilterOp::Like(_) => 4,
            FilterOp::Regex(_) => 5,
        }
    }
}

impl Query {
    pub fn optimize(&mut self) {
        self.filters.sort_by_key(|f| f.op.selectivity_rank());
    }

    pub fn matches_all(&self, row: &std::collections::HashMap<String, String>) -> bool {
        self.filters.iter().all(|f| f.matches(row))
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
    fn test_glob_match() {
        assert!(glob_match("*John*", "Hello John Doe"));
        assert!(!glob_match("*Jane*", "Hello John Doe"));
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
        // Invalid pattern should return false
        assert!(!glob_match("[invalid", "test"));
    }

    #[test]
    fn test_regex_match() {
        assert!(regex_match(r"^\d{3}-\d{4}$", "123-4567"));
        assert!(!regex_match(r"^\d{3}-\d{4}$", "12-34567"));
        // Invalid regex should return false
        assert!(!regex_match(r"[invalid", "test"));
    }

    #[test]
    fn test_filter_matches_like() {
        let query = parse_query(r#"users name~glob"*John*""#).unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].column, "name");
        match &query.filters[0].op {
            FilterOp::Like(pattern) => {
                assert_eq!(pattern, "*John*");
                assert!(glob_match(pattern, "Alice Johnson"), "glob_match");
            }
            other => panic!("expected Like, got {:?}", other),
        }
    }

    #[test]
    fn test_filter_matches_regex() {
        let query = parse_query(r#"users msg~re:"error|warn""#).unwrap();
        assert_eq!(query.filters.len(), 1);
        match &query.filters[0].op {
            FilterOp::Regex(pattern) => {
                assert_eq!(pattern, "error|warn");
                assert!(regex_match(pattern, "error: timeout"), "regex_match");
            }
            other => panic!("expected Regex, got {:?}", other),
        }
    }

    #[test]
    fn test_filter_matches_between() {
        let row: std::collections::HashMap<String, String> =
            std::collections::HashMap::from([("age".into(), "25".into())]);
        let query = parse_query("users age>=18 age<=30").unwrap();
        assert!(query.filters[0].matches(&row));
        assert!(query.filters[1].matches(&row));
        // Numeric comparison: 100.0 > 30.0, so 100 <= 30 is false
        let row2: std::collections::HashMap<String, String> =
            std::collections::HashMap::from([("age".into(), "100".into())]);
        assert!(
            !query.filters[1].matches(&row2),
            "numeric 100 <= 30 should be false"
        );

        // Non-numeric strings still use lexicographic comparison
        let row3: std::collections::HashMap<String, String> =
            std::collections::HashMap::from([("name".into(), "xyz".into())]);
        let q3 = parse_query("users name>=abc").unwrap();
        assert!(q3.filters[0].matches(&row3), "lexicographic xyz >= abc");
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

    #[test]
    fn test_like_match_literal_starts_with() {
        assert!(like_match("foo*", "foobar"));
        assert!(!like_match("foo*", "barfoo"));
    }

    #[test]
    fn test_like_match_literal_ends_with() {
        assert!(like_match("*bar", "foobar"));
        assert!(!like_match("*bar", "foobaz"));
    }

    #[test]
    fn test_like_match_literal_contains() {
        assert!(like_match("*oba*", "foobar"));
        assert!(!like_match("*xyz*", "foobar"));
    }

    #[test]
    fn test_like_match_exact() {
        assert!(like_match("hello", "hello"));
        assert!(!like_match("hello", "world"));
    }

    #[test]
    fn test_like_match_wildcard_all() {
        assert!(like_match("*", "anything"));
        assert!(like_match("*", ""));
    }

    #[test]
    fn test_like_match_regex_fallback() {
        // Patterns with wildcards in the middle need regex
        assert!(like_match("te*t", "test"));
        assert!(like_match("te*t", "tent"), "te*t matches tent (te+n+t)");
        // But does not match if prefix/suffix don't match
        assert!(!like_match("te*t", "xyz"));
        // Complex pattern
        assert!(like_match("a*b*c", "axbyc"));
        assert!(!like_match("a*b*c", "axbyz"));
    }

    #[test]
    fn test_compare_values_numeric() {
        assert!(compare_values("500", ">=", "100"));
        assert!(!compare_values("50", ">=", "100"));
        assert!(compare_values("100", "<=", "100"));
        assert!(compare_values("50", "<", "100"));
        assert!(!compare_values("100", "<", "50"));
    }

    #[test]
    fn test_compare_values_string_fallback() {
        assert!(compare_values("xyz", ">=", "abc"));
        assert!(!compare_values("abc", ">=", "xyz"));
    }

    #[test]
    fn test_compare_values_bad_op() {
        assert!(!compare_values("1", "??", "2"));
    }

    #[test]
    fn test_cached_regex() {
        let r1 = cached_regex(r"^\d+$").unwrap();
        assert!(r1.is_match("123"));
        // Same pattern should use cache
        let r2 = cached_regex(r"^\d+$").unwrap();
        assert!(r2.is_match("456"));
        // Invalid regex
        assert!(cached_regex(r"[").is_err());
    }

    #[test]
    fn test_selectivity_rank_ordering() {
        assert!(
            FilterOp::Eq("".into()).selectivity_rank() < FilterOp::Ne("".into()).selectivity_rank()
        );
        assert!(
            FilterOp::Ne("".into()).selectivity_rank()
                < FilterOp::Gte("".into()).selectivity_rank()
        );
        assert!(
            FilterOp::Gte("".into()).selectivity_rank() < FilterOp::In(vec![]).selectivity_rank()
        );
        assert!(
            FilterOp::In(vec![]).selectivity_rank() < FilterOp::Like("".into()).selectivity_rank()
        );
        assert!(
            FilterOp::Like("".into()).selectivity_rank()
                < FilterOp::Regex("".into()).selectivity_rank()
        );
    }

    #[test]
    fn test_query_optimize_reorders_filters() {
        let mut q = parse_query("users name~glob\"*abc*\" status=active age>=18").unwrap();
        assert_eq!(q.filters[0].column, "name");
        assert_eq!(q.filters[1].column, "status");
        assert_eq!(q.filters[2].column, "age");
        q.optimize();
        // Eq should come first, then range, then Like
        assert_eq!(q.filters[0].column, "status", "eq should be first");
        assert_eq!(q.filters[1].column, "age", "range should be second");
        assert_eq!(q.filters[2].column, "name", "like should be last");
    }

    #[test]
    fn test_query_matches_all() {
        let row: HashMap<String, String> = HashMap::from([
            ("status".into(), "active".into()),
            ("age".into(), "25".into()),
        ]);
        let q = parse_query("users status=active age>=18").unwrap();
        assert!(q.matches_all(&row));
        let row2: HashMap<String, String> = HashMap::from([("status".into(), "inactive".into())]);
        assert!(!q.matches_all(&row2));
    }

    #[test]
    fn test_missing_column_returns_false() {
        let row: HashMap<String, String> = HashMap::new();
        let q = parse_query("users missing=value").unwrap();
        assert!(!q.filters[0].matches(&row));
    }

    #[test]
    fn test_parse_query_source_only() {
        let q = parse_query("users").unwrap();
        assert_eq!(q.source, "users");
        assert!(q.filters.is_empty());
    }

    #[test]
    fn test_parse_query_string_literal_source() {
        let q = parse_query(r#""my source" status=active"#).unwrap();
        assert_eq!(q.source, "my source");
    }
}
