//! Streaming JSON parser for RESPONSE sections.
//!
//! Parses multiple JSON values from a single section block by tracking
//! depth, string state, and escape characters — a state-machine approach
//! that handles JSON5 with comments.

use crate::parser::json_mod;

/// Parse multiple JSON values from a single content string.
///
/// Used for streaming response sections where multiple JSON objects
/// are concatenated (e.g., NDJSON or concatenated JSON5).
///
/// Returns `Some(values)` if 2+ values were successfully parsed, `None` otherwise.
pub fn parse_response_json_values(content: &str) -> Option<Vec<serde_json::Value>> {
    let mut values = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut started = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() && current_lines.is_empty() {
            continue;
        }

        current_lines.push(line);

        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
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
