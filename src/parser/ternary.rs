// Ternary expression parser for EXTRACT section
// Converts: condition ? true_expr : false_expr
// To JQ:    if condition then true_expr else false_expr end

/// Convert ternary expression to JQ syntax
pub fn ternary_to_jq(expr: &str) -> String {
    // Simple ternary: condition ? true_expr : false_expr
    if let Some(pos) = find_top_level_question_mark(expr) {
        let (condition, rest) = expr.split_at(pos);
        let rest = &rest[1..]; // Skip '?'

        if let Some(colon_pos) = find_top_level_colon(rest) {
            let true_expr = &rest[..colon_pos].trim();
            let false_expr = &rest[colon_pos + 1..].trim();

            return format!(
                "if {} then {} else {} end",
                condition.trim(),
                true_expr,
                false_expr
            );
        }
    }

    // Not a ternary, return as-is
    expr.to_string()
}

/// Find '?' that's not inside quotes or parentheses
fn find_top_level_question_mark(expr: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = None;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;

    for (i, c) in expr.char_indices() {
        match c {
            '\'' | '"' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = Some(c);
                } else if Some(c) == quote_char {
                    in_quotes = false;
                    quote_char = None;
                }
            }
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '?' if !in_quotes && paren_depth == 0 && bracket_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

/// Find ':' that's not inside quotes or parentheses
fn find_top_level_colon(expr: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = None;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;

    for (i, c) in expr.char_indices() {
        match c {
            '\'' | '"' => {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = Some(c);
                } else if Some(c) == quote_char {
                    in_quotes = false;
                    quote_char = None;
                }
            }
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            ':' if !in_quotes && paren_depth == 0 && bracket_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

/// Process EXTRACT value - convert ternary to JQ if needed
pub fn process_extract_value(value: &str) -> String {
    // Check if it contains ternary operator
    if value.contains('?') && value.contains(':') {
        ternary_to_jq(value)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ternary_basic() {
        let input = ".status == 200 ? \"OK\" : \"Error\"";
        let expected = "if .status == 200 then \"OK\" else \"Error\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn test_ternary_with_jq() {
        let input = ".items | length > 0 ? .items[0] : null";
        let expected = "if .items | length > 0 then .items[0] else null end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn test_ternary_nested() {
        // Nested ternary - only top-level is converted
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        // Only the outer ternary is converted
        let expected = "if .a > 0 then (.a > 10 ? \"big\" : \"medium\") else \"small\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn test_not_ternary() {
        let input = ".data | .value";
        assert_eq!(ternary_to_jq(input), input);
    }

    #[test]
    fn test_ternary_in_quotes() {
        let input = ".text == \"a ? b : c\" ? \"match\" : \"no match\"";
        let expected = "if .text == \"a ? b : c\" then \"match\" else \"no match\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn test_process_extract_value_ternary() {
        let input = ".status == 200 ? \"OK\" : \"Error\"";
        let result = process_extract_value(input);
        assert!(result.starts_with("if"));
        assert!(result.ends_with("end"));
    }

    #[test]
    fn test_process_extract_value_jq() {
        let input = "if .status == 200 then \"OK\" else \"Error\" end";
        let result = process_extract_value(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_process_extract_value_simple() {
        let input = ".value";
        let result = process_extract_value(input);
        assert_eq!(result, input);
    }
}
