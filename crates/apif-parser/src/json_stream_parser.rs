use crate::json_mod;

pub fn parse_response_json_values(content: &str) -> Option<Vec<serde_json::Value>> {
    let mut values = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut started = false;
    let mut in_block_comment = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && current_lines.is_empty() {
            continue;
        }

        current_lines.push(line);

        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if in_block_comment {
                if ch == '*' && chars.next_if_eq(&'/').is_some() {
                    in_block_comment = false;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if ch == '"' {
                in_string = !in_string;
                started = true;
                continue;
            }

            if in_string {
                continue;
            }

            if ch == '#' {
                break;
            }
            if ch == '/' && chars.next_if_eq(&'/').is_some() {
                break;
            }
            if ch == '/' && chars.next_if_eq(&'*').is_some() {
                in_block_comment = true;
                continue;
            }

            match ch {
                '{' | '[' => {
                    depth += 1;
                    started = true;
                }
                '}' | ']' => {
                    depth -= 1;
                    started = true;
                    if depth < 0 {
                        return None;
                    }
                }
                c if !c.is_whitespace() => {
                    started = true;
                }
                _ => {}
            }
        }

        if started && depth == 0 {
            let chunk = current_lines.join("\n");
            let chunk = chunk.trim();
            if chunk.is_empty() {
                current_lines.clear();
                started = false;
                continue;
            }

            let value = json_mod::from_str(chunk).ok()?;
            values.push(value);
            current_lines.clear();
            started = false;
        }
    }

    if !current_lines.is_empty() {
        return None;
    }

    if values.len() >= 2 {
        Some(values)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trailing_comma_inside_a_streamed_value() {
        let content = "{\"a\": 1,}\n{\"b\": 2,}";
        let values = parse_response_json_values(content).expect("2 values");
        assert_eq!(values, vec![json!({"a": 1}), json!({"b": 2})]);
    }

    #[test]
    fn line_comment_inside_a_streamed_value() {
        let content = "{\n  \"a\": 1 // trailing comment\n}\n{\"b\": 2}";
        let values = parse_response_json_values(content).expect("2 values");
        assert_eq!(values, vec![json!({"a": 1}), json!({"b": 2})]);
    }

    #[test]
    fn block_comment_with_unbalanced_brace_inside_a_streamed_value() {
        let content = "{\n  \"a\": 1 /* note: } stray brace */\n}\n{\"b\": 2}";
        let values = parse_response_json_values(content).expect("2 values");
        assert_eq!(values, vec![json!({"a": 1}), json!({"b": 2})]);
    }
}
