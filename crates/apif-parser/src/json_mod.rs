use serde_json::Value;

/// Maximum structural nesting depth accepted before parsing.
///
/// The underlying `json5` parser is recursive, so pathologically deep input
/// (e.g. thousands of nested `[`/`{`) overflows the stack and aborts the whole
/// process — an abort that cannot be caught. This bound is far above any real
/// gRPC payload while keeping recursion safely shallow.
const MAX_JSON_DEPTH: usize = 256;

/// A JSON5 parse failure, carrying the parser's own 0-based line/column
/// (relative to the parsed content) alongside the rendered message, so a
/// caller that knows where this content starts in the file can report an
/// absolute position instead of just the section start. Comment stripping
/// (`tokenize_strip_comments`) preserves every newline, so this line always
/// lines up with the caller's own line-per-line view of the same content.
#[derive(Debug)]
pub struct JsonParseError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl std::fmt::Display for JsonParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsonParseError {}

/// Parse JSON5 string into serde_json::Value
/// Supports: comments (`//`, `#`, `/* */`), trailing commas, unquoted keys
pub fn from_str(json_str: &str) -> Result<Value, anyhow::Error> {
    // Plain JSON is the overwhelmingly common body, and JSON is a subset of
    // JSON5, so anything `serde_json` accepts parses to the same value. Taking
    // it directly skips a full-input copy in `tokenize_strip_comments` plus the
    // much slower `json5` deserializer. Comments, trailing commas, unquoted
    // keys, digit separators, `Infinity`, and over-deep input all fail here and
    // fall through to the JSON5 path below, which owns the diagnostics.
    //
    // `serde_json` has its own recursion limit, so a too-deep document errors
    // out here and still reaches the `MAX_JSON_DEPTH` check below.
    if let Ok(value) = serde_json::from_str::<Value>(json_str) {
        return Ok(value);
    }

    let (cleaned, max_depth) = tokenize_strip_comments(json_str);
    if max_depth > MAX_JSON_DEPTH {
        return Err(anyhow::anyhow!(
            "Failed to parse JSON5: nesting depth {} exceeds maximum of {}",
            max_depth,
            MAX_JSON_DEPTH
        ));
    }
    json5::from_str(&cleaned).map_err(|e| {
        let position = e.position();
        anyhow::Error::new(JsonParseError {
            message: format!("Failed to parse JSON5: {e}"),
            line: position.map(|p| p.line),
            column: position.map(|p| p.column),
        })
    })
}

/// Tokenize JSON5 content, stripping all comments.
/// This is a single-pass state machine — no regex, no string hacks.
///
/// States:
///   Normal → String → Escaped
///   Normal → LineComment (`//`, `#`) → end of line
///   Normal → BlockComment (`/*`) → `*/`
/// Returns the comment-stripped output and the maximum structural nesting
/// depth (`[`/`{` outside strings and comments) seen along the way.
fn tokenize_strip_comments(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;

    while let Some(ch) = chars.next() {
        match ch {
            // JSON5 permits both double- and single-quoted strings. Comment
            // markers (`//`, `#`, `/* */`) inside either kind of string must be
            // preserved verbatim, so track the actual opening quote and only
            // terminate on the matching one.
            '"' | '\'' => {
                let quote = ch;
                out.push(ch);
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    } else if c == quote {
                        break;
                    }
                }
            }
            '/' => {
                if let Some(kind) = chars.next_if_map(|next| match next {
                    '/' | '*' => Ok(next),
                    _ => Err(next),
                }) {
                    if kind == '/' {
                        // Line comment — skip to end of line
                        for c in chars.by_ref() {
                            if c == '\n' {
                                out.push(c);
                                break;
                            }
                        }
                    } else {
                        // Block comment — skip until */
                        loop {
                            match chars.next() {
                                Some('*') => {
                                    if chars.next_if_eq(&'/').is_some() {
                                        break;
                                    }
                                }
                                Some(c) if c == '\n' => {
                                    out.push(c);
                                }
                                Some(_) => {}
                                None => break,
                            }
                        }
                    }
                } else {
                    out.push(ch)
                }
            }
            '#' => {
                // Line comment (GCTF-style) — skip to end of line
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(c);
                        break;
                    }
                }
            }
            // Digit-separator (`1_000_000`): the `json5` crate doesn't support
            // these, so drop the `_` here and let it see a plain number.
            '_' if out.chars().next_back().is_some_and(|c| c.is_ascii_digit())
                && chars.peek().is_some_and(|c| c.is_ascii_digit()) => {}
            c => {
                match c {
                    '{' | '[' => {
                        depth += 1;
                        max_depth = max_depth.max(depth);
                    }
                    '}' | ']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                out.push(c);
            }
        }
    }

    (out, max_depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_json5_simple() {
        let input = r#"{key: "value"}"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn parse_json5_comments() {
        let input = r#"{
            // This is a comment
            key: "value" /* block comment */
        }"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn parse_json5_trailing_comma() {
        let input = r#"{
            key: "value",
        }"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn parse_json5_numeric_digit_separators() {
        let input = r#"{
            amount: 1_000_000,
            price: 1_234.567_89,
            note: "id_1_000_2 stays untouched inside a string"
        }"#;
        let expected = json!({
            "amount": 1_000_000,
            "price": 1_234.567_89,
            "note": "id_1_000_2 stays untouched inside a string"
        });
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn parse_json5_unquoted_keys() {
        let input = r#"{
            key: "value",
            number: 123,
        }"#;
        let expected = json!({
            "key": "value",
            "number": 123
        });
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn parse_hash_comments() {
        let input = r#"{
            key: "value", # inline comment
            num: 1
        }"#;
        let expected = json!({"key": "value", "num": 1});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn hash_in_string_not_comment() {
        let input = r#"{
            url: "https://example.com/path#anchor"
        }"#;
        let expected = json!({"url": "https://example.com/path#anchor"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn tokenize_inline_slash_comment() {
        let input = r#"{
  "ipsToDecorations": {
    "10.0.0.1": {
      "decoration": "web-frontend",
      // "environment": "production"
    }
  }
}"#;
        let result = from_str(input).unwrap();
        assert_eq!(
            result["ipsToDecorations"]["10.0.0.1"]["decoration"],
            "web-frontend"
        );
    }

    #[test]
    fn tokenize_trailing_comment_after_json() {
        let input = r#"{
  "key": "value"
}
// trailing comment
"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn tokenize_block_comment_multiline() {
        let input = r#"{
  /* this is
     a multiline
     block comment */
  "key": "value"
}"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn tokenize_slash_in_string_preserved() {
        let input = r#"{"url": "http://example.com", "path": "a/b/c"}"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["url"], "http://example.com");
        assert_eq!(result["path"], "a/b/c");
    }

    #[test]
    fn tokenize_escaped_quotes_in_string() {
        let input = r#"{"text": "say \"hello\" // not a comment"}"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["text"], "say \"hello\" // not a comment");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn deeply_nested_rejected_without_overflow() {
        // Regression: deeply nested input previously reached the recursive json5
        // parser and overflowed the stack (uncatchable process abort). It must
        // now be rejected with a clean error instead.
        let n = 20_000;
        let input = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let err = from_str(&input).unwrap_err().to_string();
        assert!(err.contains("nesting depth"), "unexpected error: {err}");
    }

    #[test]
    fn moderately_nested_still_parses() {
        let n = 100;
        let input = format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(from_str(&input).is_ok());
    }

    #[test]
    fn brackets_inside_string_not_counted_as_depth() {
        // Brackets inside a string must not contribute to nesting depth.
        let input = "{a: \"[[[[[[[[[[\"}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "[[[[[[[[[[");
    }

    #[test]
    fn single_quoted_string_hash_not_comment() {
        // Regression: `#` inside a single-quoted JSON5 string must not be
        // stripped as a comment.
        let input = "{a: '# not a comment'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "# not a comment");
    }

    #[test]
    fn single_quoted_string_double_slash_not_comment() {
        // Regression: `//` inside a single-quoted string (e.g. a URL) must be
        // preserved, not treated as a line comment.
        let input = "{url: 'http://example.com/path'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["url"], "http://example.com/path");
    }

    #[test]
    fn single_quoted_string_block_comment_preserved() {
        // Regression: `/* */` inside a single-quoted string must not be stripped
        // (previously silently corrupted the value).
        let input = "{a: 'has /* stars */ inside'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "has /* stars */ inside");
    }

    #[test]
    fn double_quote_inside_single_quoted_string() {
        let input = "{a: 'say \"hi\"'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "say \"hi\"");
    }

    #[test]
    fn tokenize_hash_preserves_newlines() {
        let input = "{\n  # comment line 1\n  # comment line 2\n  \"key\": \"value\"\n}";
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    // Plain JSON takes a `serde_json` fast path that skips comment stripping
    // and the JSON5 deserializer. Every JSON5-only spelling must still reach
    // the fallback and produce the same value it did before the fast path
    // existed.
    #[test]
    fn strict_json_and_json5_paths_agree() {
        let strict = from_str(r#"{"a": 1, "b": [true, null], "c": {"d": "x"}}"#).unwrap();
        let json5_spelled = from_str("{a: 1, b: [true, null,], c: {d: 'x'},}").unwrap();
        assert_eq!(strict, json5_spelled);
    }

    #[test]
    fn json5_only_spellings_still_parse() {
        assert_eq!(from_str("{a: 1}").unwrap()["a"], 1);
        assert_eq!(from_str("{\"a\": 1,}").unwrap()["a"], 1);
        assert_eq!(from_str("{\"a\": 1_000}").unwrap()["a"], 1000);
        assert_eq!(from_str("{\"a\": 1} // trailing").unwrap()["a"], 1);
        assert_eq!(from_str("{'a': 'b'}").unwrap()["a"], "b");
    }

    #[test]
    fn depth_limit_still_enforced_beyond_the_fast_path() {
        // Deeper than MAX_JSON_DEPTH: `serde_json` rejects it too, so the
        // fallback must still produce the depth diagnostic rather than a
        // generic parse error.
        let deep = format!(
            "{}{}",
            "[".repeat(MAX_JSON_DEPTH + 10),
            "]".repeat(MAX_JSON_DEPTH + 10)
        );
        let err = from_str(&deep).unwrap_err().to_string();
        assert!(
            err.contains("exceeds maximum"),
            "expected the depth diagnostic, got: {err}"
        );
    }
}
