const MAX_TERNARY_DEPTH: usize = 128;

pub fn ternary_to_jq(expr: &str) -> String {
    ternary_to_jq_depth(expr, 0)
}

fn ternary_to_jq_depth(expr: &str, depth: usize) -> String {
    if depth >= MAX_TERNARY_DEPTH {
        return expr.to_string();
    }
    if let Some(pos) = find_top_level_question_mark(expr) {
        let (condition, rest) = expr.split_at(pos);
        let rest = &rest[1..];

        if let Some(colon_pos) = find_matching_colon(rest) {
            let true_expr = &rest[..colon_pos].trim();
            let false_expr = &rest[colon_pos + 1..].trim();

            return format!(
                "if {} then {} else {} end",
                ternary_to_jq_depth(condition.trim(), depth + 1),
                ternary_to_jq_depth(true_expr, depth + 1),
                ternary_to_jq_depth(false_expr, depth + 1)
            );
        }
    }

    let mut result = String::new();
    let mut paren_depth = 0;
    let mut paren_start = None;
    let mut in_quotes = false;
    let mut quote_char = None;

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
            '(' if !in_quotes => {
                if paren_depth == 0 {
                    paren_start = Some(i);
                }
                paren_depth += 1;
            }
            ')' if !in_quotes => {
                paren_depth -= 1;
                if paren_depth == 0
                    && let Some(start) = paren_start
                {
                    let content = &expr[start + 1..i];
                    let processed = ternary_to_jq_depth(content, depth + 1);
                    result.push('(');
                    result.push_str(&processed);
                    result.push(')');
                    paren_start = None;
                    continue;
                }
            }
            _ => {}
        }

        if paren_depth == 0 {
            result.push(c);
        }
    }

    if result.is_empty() {
        expr.to_string()
    } else {
        result
    }
}

fn find_top_level_question_mark(expr: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut escaped = false;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;

    for (i, c) in expr.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote_char {
                in_quotes = false;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                in_quotes = true;
                quote_char = c;
            }
            '(' | '{' => paren_depth += 1,
            ')' | '}' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '?' if paren_depth == 0 && bracket_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

fn find_matching_colon(expr: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut escaped = false;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut ternary_depth = 0;

    for (i, c) in expr.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote_char {
                in_quotes = false;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                in_quotes = true;
                quote_char = c;
            }
            '(' | '{' => paren_depth += 1,
            ')' | '}' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '?' if paren_depth == 0 && bracket_depth == 0 => {
                ternary_depth += 1;
            }
            ':' if paren_depth == 0 && bracket_depth == 0 => {
                if ternary_depth == 0 {
                    return Some(i);
                }
                ternary_depth -= 1;
            }
            _ => {}
        }
    }

    None
}

pub fn process_extract_value(value: &str) -> String {
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
    fn ternary_basic() {
        let input = ".status == 200 ? \"OK\" : \"Error\"";
        let expected = "if .status == 200 then \"OK\" else \"Error\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn ternary_with_jq() {
        let input = ".items | length > 0 ? .items[0] : null";
        let expected = "if .items | length > 0 then .items[0] else null end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn ternary_nested() {
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let expected =
            "if .a > 0 then (if .a > 10 then \"big\" else \"medium\" end) else \"small\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn not_ternary() {
        let input = ".data | .value";
        assert_eq!(ternary_to_jq(input), input);
    }

    #[test]
    fn ternary_in_quotes() {
        let input = ".text == \"a ? b : c\" ? \"match\" : \"no match\"";
        let expected = "if .text == \"a ? b : c\" then \"match\" else \"no match\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn process_extract_value_ternary() {
        let input = ".status == 200 ? \"OK\" : \"Error\"";
        let result = process_extract_value(input);
        assert!(result.starts_with("if"));
        assert!(result.ends_with("end"));
    }

    #[test]
    fn process_extract_value_jq() {
        let input = "if .status == 200 then \"OK\" else \"Error\" end";
        let result = process_extract_value(input);
        assert_eq!(result, input);
    }

    #[test]
    fn process_extract_value_simple() {
        let input = ".value";
        let result = process_extract_value(input);
        assert_eq!(result, input);
    }

    #[test]
    fn ternary_nested_deep() {
        let input =
            ".a > 0 ? (.a > 10 ? (.a > 20 ? \"very big\" : \"big\") : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        assert!(result.contains("if .a > 0 then"));
        assert!(result.contains("if .a > 10 then"));
        assert!(result.contains("if .a > 20 then"));
        assert_eq!(result.matches(" end").count(), 3);
    }

    #[test]
    fn ternary_multiple_sequential() {
        let input = ".a > 0 ? .b > 0 ? \"both positive\" : \"a only\" : \"none\"";
        let result = ternary_to_jq(input);
        assert!(result.starts_with("if .a > 0 then"));
        assert!(result.ends_with("end"));
        println!("Sequential: {}", result);
    }

    #[test]
    fn ternary_jq_validation() {
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        assert_eq!(result.matches("if ").count(), 2);
        assert_eq!(result.matches(" then ").count(), 2);
        assert_eq!(result.matches(" else ").count(), 2);
        assert_eq!(result.matches(" end").count(), 2);
        println!("Nested jq: {}", result);
    }

    #[test]
    fn ternary_paren_inside_string_literal() {
        let input = ".name == \"a(b\" ? \"y\" : \"z\"";
        let expected = "if .name == \"a(b\" then \"y\" else \"z\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn ternary_escaped_quote_in_string() {
        let input = ".name == \"a\\\"?b\" ? \"y\" : \"z\"";
        let expected = "if .name == \"a\\\"?b\" then \"y\" else \"z\" end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn ternary_deep_nesting_no_stack_overflow() {
        let input = format!("{}.x{}", "(".repeat(100_000), ")".repeat(100_000));
        let result = ternary_to_jq(&input);
        assert!(!result.is_empty());
    }

    #[test]
    fn ternary_with_object_literal_branches() {
        let input = ".x == 0 ? {a: 1} : {b: 2}";
        let expected = "if .x == 0 then {a: 1} else {b: 2} end";
        assert_eq!(ternary_to_jq(input), expected);
    }

    #[test]
    fn ternary_with_parentheses_preserved() {
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        assert!(result.contains("(if .a > 10 then"));
        assert!(result.contains("end)"));
    }
}
