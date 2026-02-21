use console::Style;
use dissimilar::{diff, Chunk};
use serde_json::Value;
use std::fmt::Write;

/// Generates a colored diff between two JSON values
pub fn get_json_diff(expected: &Value, actual: &Value) -> String {
    let expected_str =
        serde_json::to_string_pretty(expected).unwrap_or_else(|_| expected.to_string());
    let actual_str = serde_json::to_string_pretty(actual).unwrap_or_else(|_| actual.to_string());

    let diff_chunks = diff(&expected_str, &actual_str);

    let mut output = String::new();
    let _ = writeln!(output, "Diff (Expected - / Actual +):");

    for chunk in diff_chunks {
        match chunk {
            Chunk::Equal(text) => {
                let style = Style::new().dim();
                write!(output, "{}", style.apply_to(text)).unwrap();
            }
            Chunk::Delete(text) => {
                let style = Style::new().red();
                write!(output, "{}", style.apply_to(text)).unwrap();
            }
            Chunk::Insert(text) => {
                let style = Style::new().green();
                write!(output, "{}", style.apply_to(text)).unwrap();
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_get_json_diff() {
        let expected = json!({
            "name": "Alice",
            "age": 30
        });
        let actual = json!({
            "name": "Bob",
            "age": 30
        });

        let diff = get_json_diff(&expected, &actual);
        println!("{}", diff);

        // We check that the distinct values are present.
        // The output format is now character-based diff without +/- prefixes for partial lines.
        assert!(diff.contains("Alice"));
        assert!(diff.contains("Bob"));
        assert!(diff.contains("\"age\": 30"));
    }
}
