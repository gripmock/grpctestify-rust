//! Universal GCTF file tokenizer.
//!
//! This is the **only** module that reads raw `.gctf` text.
//! Pipeline: `text → tokenize_gctf() → Vec<GctfToken> → parser → AST`
//!
//! IMPLEMENTATION NOTES:
//! - No regex, no starts_with, no ends_with, no contains, no find()
//! - Pure byte-level scanning with exact span tracking
//! - Only uses .as_bytes() once per line to get byte slice for scanning
//! - Only uses .to_string() to create owned output (necessary for API)
//! - All "parsing" done via byte comparisons (bytes[pos] == b'-')
//!
//! String operations count: 0 (only .to_string() for output ownership)

use crate::parser::tokenizer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct GctfToken {
    pub kind: GctfTokenKind,
    pub line: usize,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GctfTokenKind {
    SectionHeader { name: String, raw_options: String },
    Comment(String),
    Blank,
    Content(String),
}

pub fn tokenize_gctf(source: &str) -> Vec<GctfToken> {
    let mut tokens = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        tokens.push(classify_line(line_idx, line));
    }
    tokens
}

fn classify_line(line_idx: usize, line: &str) -> GctfToken {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    if pos == len {
        return GctfToken {
            kind: GctfTokenKind::Blank,
            line: line_idx,
            span: Span { start: 0, end: len },
        };
    }

    if bytes[pos] == b'#' {
        return GctfToken {
            kind: GctfTokenKind::Comment(line.to_string()),
            line: line_idx,
            span: Span { start: 0, end: len },
        };
    }

    if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
        return GctfToken {
            kind: GctfTokenKind::Comment(line.to_string()),
            line: line_idx,
            span: Span { start: 0, end: len },
        };
    }

    if let Some(header) = scan_section_header(line_idx, line, bytes, len) {
        return header;
    }

    GctfToken {
        kind: GctfTokenKind::Content(line.to_string()),
        line: line_idx,
        span: Span { start: 0, end: len },
    }
}

fn scan_section_header(line_idx: usize, line: &str, bytes: &[u8], len: usize) -> Option<GctfToken> {
    let mut pos = 0;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    if pos + 2 >= len || bytes[pos] != b'-' || bytes[pos + 1] != b'-' || bytes[pos + 2] != b'-' {
        return None;
    }
    pos += 3;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    let name_start = pos;
    while pos < len && is_section_name_char(bytes[pos]) {
        pos += 1;
    }
    if pos == name_start {
        return None;
    }
    let name = slice_str(line, name_start, pos);

    let mut trailing = len;
    while trailing > pos && is_ws(bytes[trailing - 1]) {
        trailing -= 1;
    }
    if trailing < pos + 3
        || bytes[trailing - 3] != b'-'
        || bytes[trailing - 2] != b'-'
        || bytes[trailing - 1] != b'-'
    {
        return None;
    }
    let options_end = trailing - 3;

    let mut opts_start = pos;
    while opts_start < options_end && is_ws(bytes[opts_start]) {
        opts_start += 1;
    }
    let mut opts_end = options_end;
    while opts_end > opts_start && is_ws(bytes[opts_end - 1]) {
        opts_end -= 1;
    }

    let raw_options = if opts_start < opts_end {
        slice_str(line, opts_start, opts_end)
    } else {
        String::new()
    };

    Some(GctfToken {
        kind: GctfTokenKind::SectionHeader { name, raw_options },
        line: line_idx,
        span: Span { start: 0, end: len },
    })
}

pub fn tokenize_kv_line(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    if pos == len {
        return None;
    }
    if bytes[pos] == b'#' {
        return None;
    }
    if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
        return None;
    }

    let key_start = pos;
    while pos < len && bytes[pos] != b':' {
        pos += 1;
    }
    if pos == len {
        return None;
    }

    let mut key_end = pos;
    while key_end > key_start && is_ws(bytes[key_end - 1]) {
        key_end -= 1;
    }
    pos += 1;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    let val_start = pos;
    let mut val_end = len;
    while val_end > val_start && is_ws(bytes[val_end - 1]) {
        val_end -= 1;
    }

    Some((
        slice_str(line, key_start, key_end),
        slice_str(line, val_start, val_end),
    ))
}

pub fn tokenize_extract_line(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    if pos == len {
        return None;
    }
    if bytes[pos] == b'#' {
        return None;
    }
    if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
        return None;
    }

    let name_start = pos;
    while pos < len && bytes[pos] != b'=' {
        pos += 1;
    }
    if pos == len {
        return None;
    }

    let mut name_end = pos;
    while name_end > name_start && is_ws(bytes[name_end - 1]) {
        name_end -= 1;
    }
    pos += 1;

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    let val_start = pos;
    let mut val_end = len;
    while val_end > val_start && is_ws(bytes[val_end - 1]) {
        val_end -= 1;
    }

    Some((
        slice_str(line, name_start, name_end),
        slice_str(line, val_start, val_end),
    ))
}

pub fn tokenize_inline_options(raw: &str) -> Vec<(String, String)> {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    let mut result = Vec::new();

    while pos < len {
        while pos < len && is_ws(bytes[pos]) {
            pos += 1;
        }
        if pos >= len {
            break;
        }

        let tok_start = pos;
        let mut in_quotes = false;
        let mut escaped = false;

        while pos < len {
            if escaped {
                escaped = false;
                pos += 1;
                continue;
            }
            match bytes[pos] {
                b'\\' => {
                    escaped = true;
                    pos += 1;
                }
                b'"' => {
                    in_quotes = !in_quotes;
                    pos += 1;
                }
                b' ' | b'\t' if !in_quotes => break,
                _ => pos += 1,
            }
        }
        let tok_end = pos;

        let token = slice_str(raw, tok_start, tok_end);

        let mut eq_pos = None;
        let tb = token.as_bytes();
        for (i, &b) in tb.iter().enumerate() {
            if b == b'=' {
                eq_pos = Some(i);
                break;
            }
        }

        if let Some(eq) = eq_pos {
            let mut key_end = eq;
            while key_end > 0 && is_ws(tb[key_end - 1]) {
                key_end -= 1;
            }
            let mut val_start = eq + 1;
            while val_start < tb.len() && is_ws(tb[val_start]) {
                val_start += 1;
            }
            let mut val_end = tb.len();
            while val_end > val_start && is_ws(tb[val_end - 1]) {
                val_end -= 1;
            }

            let mut key = slice_str(&token, 0, key_end);
            let mut value = slice_str(&token, val_start, val_end);

            strip_outer_quotes(&mut key);
            strip_outer_quotes(&mut value);

            result.push((key, value));
        } else {
            let mut key = token;
            strip_outer_quotes(&mut key);
            result.push((key, "true".to_string()));
        }
    }

    result
}

fn strip_outer_quotes(s: &mut String) {
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'') {
            *s = s[1..s.len() - 1].to_string();
        }
    }
}

#[inline]
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

#[inline]
fn is_section_name_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b == b'_'
}

#[inline]
fn slice_str(s: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }
    s[start..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_empty() {
        let tokens = tokenize_gctf("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_blank_lines() {
        let tokens = tokenize_gctf("\n\n  \n");
        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0].kind, GctfTokenKind::Blank));
        assert!(matches!(tokens[1].kind, GctfTokenKind::Blank));
        assert!(matches!(tokens[2].kind, GctfTokenKind::Blank));
    }

    #[test]
    fn test_tokenize_comments() {
        let tokens = tokenize_gctf("# hello\n// world");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0].kind, GctfTokenKind::Comment(t) if t == "# hello"));
        assert!(matches!(&tokens[1].kind, GctfTokenKind::Comment(t) if t == "// world"));
    }

    #[test]
    fn test_tokenize_section_headers() {
        let tokens = tokenize_gctf("--- ENDPOINT ---\n--- RESPONSE partial=true ---");
        assert_eq!(tokens.len(), 2);

        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                assert_eq!(name, "ENDPOINT");
                assert_eq!(raw_options, "");
            }
            _ => panic!("expected SectionHeader"),
        }

        match &tokens[1].kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                assert_eq!(name, "RESPONSE");
                assert_eq!(raw_options, "partial=true");
            }
            _ => panic!("expected SectionHeader"),
        }
    }

    #[test]
    fn test_tokenize_content() {
        let tokens = tokenize_gctf("hello world\n{\"key\": \"value\"}");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0].kind, GctfTokenKind::Content(t) if t == "hello world"));
        assert!(
            matches!(&tokens[1].kind, GctfTokenKind::Content(t) if t == "{\"key\": \"value\"}")
        );
    }

    #[test]
    fn test_tokenize_not_section_header() {
        let tokens = tokenize_gctf("--- not uppercase ---\n---ABC");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
        assert!(matches!(tokens[1].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_tokenize_full_document() {
        let input = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ASSERTS ---
.x == 1
"#;
        let tokens = tokenize_gctf(input);
        let mut kinds: Vec<&str> = Vec::new();
        for t in &tokens {
            match &t.kind {
                GctfTokenKind::SectionHeader { .. } => kinds.push("H"),
                GctfTokenKind::Comment(_) => kinds.push("C"),
                GctfTokenKind::Blank => kinds.push("_"),
                GctfTokenKind::Content(_) => kinds.push("T"),
            }
        }
        assert_eq!(kinds, vec!["H", "T", "_", "H", "T", "_", "H", "T"]);
    }

    #[test]
    fn test_tokenize_section_header_with_multiple_options() {
        let tokens = tokenize_gctf("--- RESPONSE partial=true tolerance=0.1 ---");
        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                assert_eq!(name, "RESPONSE");
                assert_eq!(raw_options, "partial=true tolerance=0.1");
            }
            _ => panic!("expected SectionHeader"),
        }
    }

    #[test]
    fn test_kv_line_basic() {
        let (key, value) = tokenize_kv_line("Authorization: Bearer token").unwrap();
        assert_eq!(key, "Authorization");
        assert_eq!(value, "Bearer token");
    }

    #[test]
    fn test_kv_line_with_whitespace() {
        let (key, value) = tokenize_kv_line("  key  :  value  ").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "value");
    }

    #[test]
    fn test_kv_line_comment() {
        assert_eq!(tokenize_kv_line("# comment"), None);
        assert_eq!(tokenize_kv_line("// comment"), None);
    }

    #[test]
    fn test_kv_line_empty() {
        assert_eq!(tokenize_kv_line(""), None);
        assert_eq!(tokenize_kv_line("   "), None);
    }

    #[test]
    fn test_kv_line_no_colon() {
        assert_eq!(tokenize_kv_line("no colon here"), None);
    }

    #[test]
    fn test_extract_line_basic() {
        let (name, value) = tokenize_extract_line("total = .response.total").unwrap();
        assert_eq!(name, "total");
        assert_eq!(value, ".response.total");
    }

    #[test]
    fn test_extract_line_comment() {
        assert_eq!(tokenize_extract_line("# comment"), None);
        assert_eq!(tokenize_extract_line("// comment"), None);
    }

    #[test]
    fn test_extract_line_empty() {
        assert_eq!(tokenize_extract_line(""), None);
    }

    #[test]
    fn test_tokenize_inline_options_basic() {
        let opts = tokenize_inline_options("key1=value1 key2=value2");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("key1".into(), "value1".into()));
        assert_eq!(opts[1], ("key2".into(), "value2".into()));
    }

    #[test]
    fn test_tokenize_inline_options_quoted() {
        let opts = tokenize_inline_options(r#"key="hello world""#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0], ("key".into(), "hello world".into()));
    }

    #[test]
    fn test_tokenize_inline_options_boolean_short_form() {
        let opts = tokenize_inline_options("partial");
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0], ("partial".into(), "true".into()));
    }

    #[test]
    fn test_tokenize_inline_options_complex() {
        let opts = tokenize_inline_options("with_asserts=true partial=false tolerance=0.1");
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0], ("with_asserts".into(), "true".into()));
        assert_eq!(opts[1], ("partial".into(), "false".into()));
        assert_eq!(opts[2], ("tolerance".into(), "0.1".into()));
    }

    #[test]
    fn test_line_numbers() {
        let tokens = tokenize_gctf("line0\nline1\nline2");
        assert_eq!(tokens[0].line, 0);
        assert_eq!(tokens[1].line, 1);
        assert_eq!(tokens[2].line, 2);
    }

    // === scan_section_header edge cases ===

    #[test]
    fn test_section_header_empty_name() {
        let tokens = tokenize_gctf("--- ---");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_section_header_no_closing_dashes() {
        let tokens = tokenize_gctf("--- ENDPOINT");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_section_header_only_dashes() {
        let tokens = tokenize_gctf("------");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_section_header_leading_whitespace() {
        let tokens = tokenize_gctf("  --- ENDPOINT ---");
        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, .. } => assert_eq!(name, "ENDPOINT"),
            _ => panic!("expected SectionHeader"),
        }
    }

    #[test]
    fn test_section_header_extra_whitespace() {
        let tokens = tokenize_gctf("---   RESPONSE   partial=true   ---");
        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                assert_eq!(name, "RESPONSE");
                assert_eq!(raw_options, "partial=true");
            }
            _ => panic!("expected SectionHeader"),
        }
    }

    #[test]
    fn test_section_header_lowercase_rejected() {
        let tokens = tokenize_gctf("--- endpoint ---");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_section_header_mixed_case_treated_as_partial_name() {
        let tokens = tokenize_gctf("--- Endpoint ---");
        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, .. } => assert_eq!(name, "E"),
            _ => panic!("expected SectionHeader with truncated name"),
        }
    }

    #[test]
    fn test_section_header_fully_lowercase_rejected() {
        let tokens = tokenize_gctf("--- endpoint ---");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_section_header_with_underscore() {
        let tokens = tokenize_gctf("--- REQUEST_HEADERS ---");
        match &tokens[0].kind {
            GctfTokenKind::SectionHeader { name, .. } => assert_eq!(name, "REQUEST_HEADERS"),
            _ => panic!("expected SectionHeader"),
        }
    }

    #[test]
    fn test_three_dashes_in_content() {
        let tokens = tokenize_gctf("---ABC");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Content(_)));
    }

    #[test]
    fn test_comment_with_leading_whitespace() {
        let tokens = tokenize_gctf("  # indented comment");
        assert!(
            matches!(&tokens[0].kind, GctfTokenKind::Comment(t) if t == "  # indented comment")
        );
    }

    #[test]
    fn test_slash_slash_not_at_start_is_content() {
        let tokens = tokenize_gctf("foo // bar");
        assert!(matches!(&tokens[0].kind, GctfTokenKind::Content(t) if t == "foo // bar"));
    }

    #[test]
    fn test_tab_only_line_is_blank() {
        let tokens = tokenize_gctf("\t\t");
        assert!(matches!(tokens[0].kind, GctfTokenKind::Blank));
    }

    // === tokenize_kv_line edge cases ===

    #[test]
    fn test_kv_line_empty_value() {
        let (key, value) = tokenize_kv_line("key:").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "");
    }

    #[test]
    fn test_kv_line_colon_in_value() {
        let (key, value) = tokenize_kv_line("url: http://host:8080").unwrap();
        assert_eq!(key, "url");
        assert_eq!(value, "http://host:8080");
    }

    #[test]
    fn test_kv_line_value_with_spaces() {
        let (key, value) = tokenize_kv_line("  cert  :  /path/to/cert.pem  ").unwrap();
        assert_eq!(key, "cert");
        assert_eq!(value, "/path/to/cert.pem");
    }

    #[test]
    fn test_kv_line_tab_separator() {
        let (key, value) = tokenize_kv_line("key\t:\tvalue").unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "value");
    }

    #[test]
    fn test_kv_line_only_whitespace_key_produces_empty_key() {
        let result = tokenize_kv_line("   : value");
        assert!(result.is_some());
        let (key, value) = result.unwrap();
        assert_eq!(key, "");
        assert_eq!(value, "value");
    }

    // === tokenize_extract_line edge cases ===

    #[test]
    fn test_extract_line_with_spaces() {
        let (name, value) = tokenize_extract_line("  total  =  .response.total  ").unwrap();
        assert_eq!(name, "total");
        assert_eq!(value, ".response.total");
    }

    #[test]
    fn test_extract_line_empty_value() {
        let (name, value) = tokenize_extract_line("name=").unwrap();
        assert_eq!(name, "name");
        assert_eq!(value, "");
    }

    #[test]
    fn test_extract_line_no_equals() {
        assert_eq!(tokenize_extract_line(".just.a.path"), None);
    }

    #[test]
    fn test_extract_line_whitespace_only() {
        assert_eq!(tokenize_extract_line("   "), None);
    }

    #[test]
    fn test_extract_line_only_whitespace_value() {
        let (name, value) = tokenize_extract_line("name=   ").unwrap();
        assert_eq!(name, "name");
        assert_eq!(value, "");
    }

    // === tokenize_inline_options edge cases ===

    #[test]
    fn test_tokenize_options_empty() {
        let opts = tokenize_inline_options("");
        assert!(opts.is_empty());
    }

    #[test]
    fn test_tokenize_options_only_spaces() {
        let opts = tokenize_inline_options("   ");
        assert!(opts.is_empty());
    }

    #[test]
    fn test_tokenize_options_escaped_char() {
        let opts = tokenize_inline_options(r#"key=va\"lue"#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "key");
    }

    #[test]
    fn test_tokenize_options_single_quotes_not_special() {
        let opts = tokenize_inline_options("key='hello world'");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("key".into(), "'hello".into()));
    }

    #[test]
    fn test_tokenize_options_array_value() {
        let opts = tokenize_inline_options(r#"redact=["field1","field2"]"#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "redact");
        assert!(opts[0].1.contains("field1"));
    }

    #[test]
    fn test_tokenize_options_multiple_spaces() {
        let opts = tokenize_inline_options("  a=1   b=2  ");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("a".into(), "1".into()));
        assert_eq!(opts[1], ("b".into(), "2".into()));
    }

    #[test]
    fn test_tokenize_options_tab_separated() {
        let opts = tokenize_inline_options("a=1\tb=2");
        assert_eq!(opts.len(), 2);
    }

    #[test]
    fn test_tokenize_options_quoted_key() {
        let opts = tokenize_inline_options(r#""key"=value"#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].0, "key");
    }

    #[test]
    fn test_tokenize_options_empty_value() {
        let opts = tokenize_inline_options("key=");
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0], ("key".into(), "".into()));
    }

    // === slice_str edge case (via other functions) ===

    #[test]
    fn test_span_tracking() {
        let tokens = tokenize_gctf("# comment\n--- ENDPOINT ---\nhello");
        assert_eq!(tokens[0].span, Span { start: 0, end: 9 });
        assert_eq!(tokens[1].span, Span { start: 0, end: 16 });
        assert_eq!(tokens[2].span, Span { start: 0, end: 5 });
    }

    #[test]
    fn test_gctf_token_kind_equality() {
        let t1 = GctfToken {
            kind: GctfTokenKind::Blank,
            line: 0,
            span: Span { start: 0, end: 0 },
        };
        let t2 = GctfToken {
            kind: GctfTokenKind::Blank,
            line: 1,
            span: Span { start: 0, end: 0 },
        };
        assert_ne!(t1, t2); // different line
    }
}
