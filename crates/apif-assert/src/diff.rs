use console::Style;
use serde_json::Value;
use std::fmt::Write;

const CONTEXT: usize = 3;

const FULL_LIMIT: usize = 60;

const PAIR_LIMIT: usize = 250_000;

enum Edit<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

pub fn get_json_diff(expected: &Value, actual: &Value) -> String {
    let expected_str =
        serde_json::to_string_pretty(expected).unwrap_or_else(|_| expected.to_string());
    let actual_str = serde_json::to_string_pretty(actual).unwrap_or_else(|_| actual.to_string());

    let expected_lines: Vec<&str> = expected_str.lines().collect();
    let actual_lines: Vec<&str> = actual_str.lines().collect();
    let edits = line_edits(&expected_lines, &actual_lines);

    let mut output = String::new();
    let _ = writeln!(output, "Diff (Expected - / Actual +):");
    render(&edits, &mut output);
    output
}

fn render(edits: &[Edit<'_>], output: &mut String) {
    let dim = Style::new().dim();
    let red = Style::new().red();
    let green = Style::new().green();
    let long = edits.len() > FULL_LIMIT;

    let mut i = 0;
    while i < edits.len() {
        let Edit::Same(_) = edits[i] else {
            match &edits[i] {
                Edit::Removed(line) => {
                    let _ = writeln!(output, "{}", red.apply_to(format!("- {line}")));
                }
                Edit::Added(line) => {
                    let _ = writeln!(output, "{}", green.apply_to(format!("+ {line}")));
                }
                Edit::Same(_) => unreachable!(),
            }
            i += 1;
            continue;
        };

        let run_end = run_of_same(edits, i);
        let run = run_end - i;
        if !long || run <= CONTEXT * 2 + 1 {
            for edit in &edits[i..run_end] {
                if let Edit::Same(line) = edit {
                    let _ = writeln!(output, "{}", dim.apply_to(format!("  {line}")));
                }
            }
            i = run_end;
            continue;
        }

        let head = if i == 0 { 0 } else { CONTEXT };
        let tail = if run_end == edits.len() { 0 } else { CONTEXT };
        for edit in &edits[i..i + head] {
            if let Edit::Same(line) = edit {
                let _ = writeln!(output, "{}", dim.apply_to(format!("  {line}")));
            }
        }
        let hidden = run - head - tail;
        let _ = writeln!(
            output,
            "{}",
            dim.apply_to(format!(
                "  … {hidden} unchanged line{}",
                if hidden == 1 { "" } else { "s" }
            ))
        );
        for edit in &edits[run_end - tail..run_end] {
            if let Edit::Same(line) = edit {
                let _ = writeln!(output, "{}", dim.apply_to(format!("  {line}")));
            }
        }
        i = run_end;
    }
}

fn run_of_same(edits: &[Edit<'_>], from: usize) -> usize {
    let mut end = from;
    while end < edits.len() && matches!(edits[end], Edit::Same(_)) {
        end += 1;
    }
    end
}

fn line_edits<'a>(expected: &[&'a str], actual: &[&'a str]) -> Vec<Edit<'a>> {
    let head = expected
        .iter()
        .zip(actual.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = expected[head..]
        .iter()
        .rev()
        .zip(actual[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let mut edits: Vec<Edit<'a>> = expected[..head].iter().map(|l| Edit::Same(l)).collect();
    let middle_expected = &expected[head..expected.len() - tail];
    let middle_actual = &actual[head..actual.len() - tail];

    if middle_expected.len() * middle_actual.len() <= PAIR_LIMIT {
        edits.extend(paired(middle_expected, middle_actual));
    } else {
        edits.extend(middle_expected.iter().map(|l| Edit::Removed(l)));
        edits.extend(middle_actual.iter().map(|l| Edit::Added(l)));
    }

    edits.extend(
        expected[expected.len() - tail..]
            .iter()
            .map(|l| Edit::Same(l)),
    );
    edits
}

fn paired<'a>(expected: &[&'a str], actual: &[&'a str]) -> Vec<Edit<'a>> {
    let (n, m) = (expected.len(), actual.len());
    let mut table = vec![0usize; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if expected[i] == actual[j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }

    let mut edits = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if expected[i] == actual[j] {
            edits.push(Edit::Same(expected[i]));
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            edits.push(Edit::Removed(expected[i]));
            i += 1;
        } else {
            edits.push(Edit::Added(actual[j]));
            j += 1;
        }
    }
    edits.extend(expected[i..].iter().map(|l| Edit::Removed(l)));
    edits.extend(actual[j..].iter().map(|l| Edit::Added(l)));
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain(expected: &Value, actual: &Value) -> String {
        console::strip_ansi_codes(&get_json_diff(expected, actual)).to_string()
    }

    fn marked(diff: &str, mark: char) -> Vec<String> {
        diff.lines()
            .skip(1)
            .filter(|l| l.starts_with(mark))
            .map(|l| l[1..].trim().to_string())
            .collect()
    }

    #[test]
    fn a_changed_value_keeps_both_sides_on_their_own_lines() {
        let diff = plain(&json!({"status": "down"}), &json!({"status": "ok"}));

        assert_eq!(marked(&diff, '-'), vec!["\"status\": \"down\""], "{diff}");
        assert_eq!(marked(&diff, '+'), vec!["\"status\": \"ok\""], "{diff}");
        assert!(!diff.contains("downok"), "{diff}");
    }

    #[test]
    fn what_both_sides_share_is_context() {
        let diff = plain(
            &json!({"name": "Alice", "age": 30}),
            &json!({"name": "Bob", "age": 30}),
        );

        assert!(diff.contains("    \"age\": 30"), "{diff}");
        assert_eq!(marked(&diff, '-'), vec!["\"name\": \"Alice\","], "{diff}");
        assert_eq!(marked(&diff, '+'), vec!["\"name\": \"Bob\","], "{diff}");
    }

    #[test]
    fn a_field_only_one_side_has_is_marked_once() {
        let diff = plain(&json!({"a": 1}), &json!({"a": 1, "b": 2}));

        assert_eq!(marked(&diff, '+'), vec!["\"a\": 1,", "\"b\": 2"], "{diff}");
        assert_eq!(marked(&diff, '-'), vec!["\"a\": 1"], "{diff}");
    }

    #[test]
    fn nothing_to_say_about_two_equal_values() {
        let diff = plain(&json!({"a": 1}), &json!({"a": 1}));

        assert!(marked(&diff, '-').is_empty(), "{diff}");
        assert!(marked(&diff, '+').is_empty(), "{diff}");
    }

    #[test]
    fn a_long_agreement_is_counted_rather_than_printed() {
        let rows: Vec<Value> = (0..80).map(|i| json!({"id": i})).collect();
        let mut changed = rows.clone();
        changed[79] = json!({"id": 999});

        let diff = plain(&json!(rows), &json!(changed));

        assert!(diff.contains("unchanged lines"), "{diff}");
        assert_eq!(marked(&diff, '-'), vec!["\"id\": 79"], "{diff}");
        assert_eq!(marked(&diff, '+'), vec!["\"id\": 999"], "{diff}");
        assert!(diff.lines().count() < 40, "{diff}");
    }

    #[test]
    fn two_bodies_that_share_nothing_are_still_read_side_by_side() {
        let diff = plain(&json!({"a": 1}), &json!([1, 2, 3]));

        assert!(marked(&diff, '-').contains(&"{".to_string()), "{diff}");
        assert!(marked(&diff, '+').contains(&"[".to_string()), "{diff}");
    }
}
