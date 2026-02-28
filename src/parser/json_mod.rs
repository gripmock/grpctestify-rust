use serde_json::Value;

/// Parse JSON5 string into serde_json::Value
/// This supports comments, trailing commas, and unquoted keys
pub fn from_str(json_str: &str) -> Result<Value, anyhow::Error> {
    let normalized = strip_hash_comments(json_str);
    json5::from_str(&normalized).map_err(|e| anyhow::anyhow!("Failed to parse JSON5: {}", e))
}

fn strip_hash_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());

    for line in input.lines() {
        let mut in_string = false;
        let mut escaped = false;

        for ch in line.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
                continue;
            }

            if ch == '\\' {
                out.push(ch);
                escaped = true;
                continue;
            }

            if ch == '"' {
                in_string = !in_string;
                out.push(ch);
                continue;
            }

            if ch == '#' && !in_string {
                break;
            }

            out.push(ch);
        }

        out.push('\n');
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
}
