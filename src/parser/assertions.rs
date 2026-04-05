pub(crate) fn strip_assertion_comments(line: &str) -> Option<String> {
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

        if ch == '/' && matches!(chars.peek(), Some('/')) {
            let prev_is_whitespace = out.chars().last().is_none_or(char::is_whitespace);
            if !saw_non_whitespace || prev_is_whitespace {
                break;
            }
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
    fn strips_full_line_double_slash_comments() {
        assert_eq!(strip_assertion_comments("// comment"), None);
        assert_eq!(strip_assertion_comments("   // comment"), None);
    }

    #[test]
    fn strips_inline_comments() {
        assert_eq!(
            strip_assertion_comments("@elapsed_ms() >= 10 // startup delay"),
            Some("@elapsed_ms() >= 10".to_string())
        );
        assert_eq!(
            strip_assertion_comments("@scope_message_count() == 2 # two messages"),
            Some("@scope_message_count() == 2".to_string())
        );
    }

    #[test]
    fn keeps_double_slash_inside_string() {
        assert_eq!(
            strip_assertion_comments("@regex(.url, \"^https://example.com\")"),
            Some("@regex(.url, \"^https://example.com\")".to_string())
        );
    }
}
