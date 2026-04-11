use serde_json::Value;

/// Parse JSON5 string into serde_json::Value
/// Supports: comments (`//`, `#`, `/* */`), trailing commas, unquoted keys
pub fn from_str(json_str: &str) -> Result<Value, anyhow::Error> {
    let cleaned = tokenize_strip_comments(json_str);
    json5::from_str(&cleaned).map_err(|e| anyhow::anyhow!("Failed to parse JSON5: {}", e))
}

/// Tokenize JSON5 content, stripping all comments.
/// This is a single-pass state machine — no regex, no string hacks.
///
/// States:
///   Normal → String → Escaped
///   Normal → LineComment (`//`, `#`) → end of line
///   Normal → BlockComment (`/*`) → `*/`
fn tokenize_strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                out.push(ch);
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '/' => {
                if let Some(&next) = chars.peek() {
                    match next {
                        '/' => {
                            // Line comment — skip to end of line
                            chars.next();
                            for c in chars.by_ref() {
                                if c == '\n' {
                                    out.push(c);
                                    break;
                                }
                            }
                        }
                        '*' => {
                            // Block comment — skip until */
                            chars.next();
                            loop {
                                match chars.next() {
                                    Some('*') => {
                                        if let Some(&'/') = chars.peek() {
                                            chars.next();
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
                        _ => {
                            out.push(ch);
                        }
                    }
                } else {
                    out.push(ch);
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
                out.push(c);
            }
        }
    }

    out
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
    fn test_tokenize_hash_preserves_newlines() {
        let input = "{\n  # comment line 1\n  # comment line 2\n  \"key\": \"value\"\n}";
        let result = from_str(input).unwrap();
        assert_eq!(result["key"], "value");
    }
}
