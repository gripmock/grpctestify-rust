use serde_json::Value;

/// Maximum structural nesting depth accepted before parsing.
///
/// The underlying `json5` parser is recursive, so pathologically deep input
/// (e.g. thousands of nested `[`/`{`) overflows the stack and aborts the whole
/// process — an abort that cannot be caught. This bound is far above any real
/// gRPC payload while keeping recursion safely shallow.
const MAX_JSON_DEPTH: usize = 256;

/// Parse JSON5 string into serde_json::Value
/// Supports: comments (`//`, `#`, `/* */`), trailing commas, unquoted keys
pub fn from_str(json_str: &str) -> Result<Value, anyhow::Error> {
    let (cleaned, max_depth) = tokenize_strip_comments(json_str);
    if max_depth > MAX_JSON_DEPTH {
        return Err(anyhow::anyhow!(
            "Failed to parse JSON5: nesting depth {} exceeds maximum of {}",
            max_depth,
            MAX_JSON_DEPTH
        ));
    }
    json5::from_str(&cleaned).map_err(|e| anyhow::anyhow!("Failed to parse JSON5: {}", e))
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
    fn test_parse_json5_simple() {
        let input = r#"{key: "value"}"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn test_parse_json5_comments() {
        let input = r#"{
            // This is a comment
            key: "value" /* block comment */
        }"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn test_parse_json5_trailing_comma() {
        let input = r#"{
            key: "value",
        }"#;
        let expected = json!({"key": "value"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn test_parse_json5_unquoted_keys() {
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
    fn test_parse_hash_comments() {
        let input = r#"{
            key: "value", # inline comment
            num: 1
        }"#;
        let expected = json!({"key": "value", "num": 1});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn test_hash_in_string_not_comment() {
        let input = r#"{
            url: "https://example.com/path#anchor"
        }"#;
        let expected = json!({"url": "https://example.com/path#anchor"});
        assert_eq!(from_str(input).unwrap(), expected);
    }

    #[test]
    fn test_tokenize_inline_slash_comment() {
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
    fn test_tokenize_trailing_comment_after_json() {
        let input = r#"{
  "key": "value"
}
// trailing comment
"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn test_tokenize_block_comment_multiline() {
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
    fn test_tokenize_slash_in_string_preserved() {
        let input = r#"{"url": "http://example.com", "path": "a/b/c"}"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["url"], "http://example.com");
        assert_eq!(result["path"], "a/b/c");
    }

    #[test]
    fn test_tokenize_escaped_quotes_in_string() {
        let input = r#"{"text": "say \"hello\" // not a comment"}"#;
        let result = from_str(input).unwrap();
        assert_eq!(result["text"], "say \"hello\" // not a comment");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_deeply_nested_rejected_without_overflow() {
        // Regression: deeply nested input previously reached the recursive json5
        // parser and overflowed the stack (uncatchable process abort). It must
        // now be rejected with a clean error instead.
        let n = 20_000;
        let input = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let err = from_str(&input).unwrap_err().to_string();
        assert!(err.contains("nesting depth"), "unexpected error: {err}");
    }

    #[test]
    fn test_moderately_nested_still_parses() {
        let n = 100;
        let input = format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(from_str(&input).is_ok());
    }

    #[test]
    fn test_brackets_inside_string_not_counted_as_depth() {
        // Brackets inside a string must not contribute to nesting depth.
        let input = "{a: \"[[[[[[[[[[\"}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "[[[[[[[[[[");
    }

    #[test]
    fn test_single_quoted_string_hash_not_comment() {
        // Regression: `#` inside a single-quoted JSON5 string must not be
        // stripped as a comment.
        let input = "{a: '# not a comment'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "# not a comment");
    }

    #[test]
    fn test_single_quoted_string_double_slash_not_comment() {
        // Regression: `//` inside a single-quoted string (e.g. a URL) must be
        // preserved, not treated as a line comment.
        let input = "{url: 'http://example.com/path'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["url"], "http://example.com/path");
    }

    #[test]
    fn test_single_quoted_string_block_comment_preserved() {
        // Regression: `/* */` inside a single-quoted string must not be stripped
        // (previously silently corrupted the value).
        let input = "{a: 'has /* stars */ inside'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "has /* stars */ inside");
    }

    #[test]
    fn test_double_quote_inside_single_quoted_string() {
        let input = "{a: 'say \"hi\"'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "say \"hi\"");
    }

    #[test]
    fn test_tokenize_hash_preserves_newlines() {
        let input = "{\n  # comment line 1\n  # comment line 2\n  \"key\": \"value\"\n}";
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }
}
