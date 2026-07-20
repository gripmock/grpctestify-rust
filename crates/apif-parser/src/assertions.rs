pub fn strip_assertion_comments(line: &str) -> Option<String> {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut quote_char = '\0';
    let mut escaped = false;
    let mut saw_non_whitespace = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote_char {
                in_string = false;
                quote_char = '\0';
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = true;
            quote_char = ch;
            out.push(ch);
            saw_non_whitespace = true;
            continue;
        }

        if ch == '#' {
            break;
        }

        if ch == '/' && chars.next_if_eq(&'/').is_some() {
            // Full-line comment: nothing but whitespace before `//`.
            if !saw_non_whitespace {
                break;
            }
            let prev_is_whitespace = out.chars().last().is_none_or(char::is_whitespace);
            if prev_is_whitespace {
                // Ambiguity: ` // ` may start an inline comment or be jq's
                // alternative operator (`.value // "default"`). Keep it as jq
                // `//` when the next token looks like the start of a jq
                // expression; otherwise strip it as a comment.
                let next = chars.clone().find(|c| !c.is_whitespace());
                let looks_like_jq_operand = next.is_some_and(|c| {
                    c.is_ascii_digit()
                        || matches!(c, '.' | '$' | '"' | '\'' | '(' | '[' | '{' | '@')
                });
                if !looks_like_jq_operand {
                    break;
                }
            }
            // Not a comment: keep both slashes (jq `//` alternative operator,
            // or `//` glued between operands like `1//2`).
            out.push('/');
            out.push('/');
            continue;
        }

        if !ch.is_whitespace() {
            saw_non_whitespace = true;
        }
        out.push(ch);
    }

    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::strip_assertion_comments;

    #[test]
    fn test_strips_full_line_double_slash_comments() {
        assert_eq!(strip_assertion_comments("// comment"), None);
        assert_eq!(strip_assertion_comments("   // comment"), None);
    }

    #[test]
    fn test_strips_inline_comments() {
        assert_eq!(
            strip_assertion_comments("@elapsed_ms() >= 10 // startup delay"),
            Some("@elapsed_ms() >= 10".to_string())
        );
        assert_eq!(
            strip_assertion_comments("@scope.message_count() == 2 # two messages"),
            Some("@scope.message_count() == 2".to_string())
        );
    }

    #[test]
    fn test_preserves_jq_alternative_operator() {
        // Regression: ` // ` followed by a jq operand is jq's alternative
        // operator, not a comment.
        assert_eq!(
            strip_assertion_comments(".value // \"default\" == \"x\""),
            Some(".value // \"default\" == \"x\"".to_string())
        );
        assert_eq!(
            strip_assertion_comments(".a // .b == 1"),
            Some(".a // .b == 1".to_string())
        );
    }

    #[test]
    fn test_preserves_double_slash_between_operands() {
        // Regression: `1//2` used to lose a slash and become `1/2`.
        assert_eq!(
            strip_assertion_comments(".a == 1//2"),
            Some(".a == 1//2".to_string())
        );
    }

    #[test]
    fn test_keeps_double_slash_inside_string() {
        assert_eq!(
            strip_assertion_comments("@regex(.url, \"^https://example.com\")"),
            Some("@regex(.url, \"^https://example.com\")".to_string())
        );
    }
}
