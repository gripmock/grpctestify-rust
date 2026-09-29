use serde_json::Value;

const MAX_JSON_DEPTH: usize = 256;

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

pub fn from_str(json_str: &str) -> Result<Value, anyhow::Error> {
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

fn tokenize_strip_comments(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;

    while let Some(ch) = chars.next() {
        match ch {
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
                        for c in chars.by_ref() {
                            if c == '\n' {
                                out.push(c);
                                break;
                            }
                        }
                    } else {
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
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(c);
                        break;
                    }
                }
            }
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
        let input = "{a: \"[[[[[[[[[[\"}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "[[[[[[[[[[");
    }

    #[test]
    fn single_quoted_string_hash_not_comment() {
        let input = "{a: '# not a comment'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["a"], "# not a comment");
    }

    #[test]
    fn single_quoted_string_double_slash_not_comment() {
        let input = "{url: 'http://example.com/path'}";
        let result = from_str(input).unwrap();
        assert_eq!(result["url"], "http://example.com/path");
    }

    #[test]
    fn single_quoted_string_block_comment_preserved() {
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
