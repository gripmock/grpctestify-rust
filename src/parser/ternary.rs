// Ternary expression parser for EXTRACT section
// Converts: condition ? true_expr : false_expr
// To JQ:    if condition then true_expr else false_expr end

/// Convert ternary expression to JQ syntax (recursively handles nested ternaries)
pub fn ternary_to_jq(expr: &str) -> String {
    // First, check if the entire expression is a ternary
    if let Some(pos) = find_top_level_question_mark(expr) {
        let (condition, rest) = expr.split_at(pos);
        let rest = &rest[1..]; // Skip '?'

        if let Some(colon_pos) = find_matching_colon(rest) {
            let true_expr = &rest[..colon_pos].trim();
            let false_expr = &rest[colon_pos + 1..].trim();

            // Recursively process nested ternaries in all parts
            return format!(
                "if {} then {} else {} end",
                ternary_to_jq(condition.trim()),
                ternary_to_jq(true_expr),
                ternary_to_jq(false_expr)
            );
        }
    }

    // Not a top-level ternary, but may contain nested ternaries in parentheses
    // Process content inside parentheses recursively
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
                    // Process content inside parentheses
                    let content = &expr[start + 1..i];
                    let processed = ternary_to_jq(content);
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

/// Find ':' that matches the first '?' (handles right-associative ternaries)
/// For "a ? b ? c : d : e", finds the second ':' (position after d)
fn find_matching_colon(expr: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_char = None;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut ternary_depth = 0;  // Count nested ? without matching :

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
                ternary_depth += 1;
            }
            ':' if !in_quotes && paren_depth == 0 && bracket_depth == 0 => {
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
        // Nested ternary - all levels are recursively converted
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let expected = "if .a > 0 then (if .a > 10 then \"big\" else \"medium\" end) else \"small\" end";
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

    #[test]
    fn test_ternary_nested_deep() {
        // Deep nested: 3 levels
        let input = ".a > 0 ? (.a > 10 ? (.a > 20 ? \"very big\" : \"big\") : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        assert!(result.contains("if .a > 0 then"));
        assert!(result.contains("if .a > 10 then"));
        assert!(result.contains("if .a > 20 then"));
        assert_eq!(result.matches(" end").count(), 3);
    }

    #[test]
    fn test_ternary_multiple_sequential() {
        // Multiple ternaries at same level (not nested)
        let input = ".a > 0 ? .b > 0 ? \"both positive\" : \"a only\" : \"none\"";
        let result = ternary_to_jq(input);
        // Should handle right-associative: a>0 ? (b>0 ? ... : ...) : ...
        assert!(result.starts_with("if .a > 0 then"));
        assert!(result.ends_with("end"));
        println!("Sequential: {}", result);
    }

    #[test]
    fn test_ternary_jq_validation() {
        // Verify generated jq is valid by checking structure
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        // Count if/then/else/end balance
        assert_eq!(result.matches("if ").count(), 2);
        assert_eq!(result.matches(" then ").count(), 2);
        assert_eq!(result.matches(" else ").count(), 2);
        assert_eq!(result.matches(" end").count(), 2);
        println!("Nested jq: {}", result);
    }

    #[test]
    fn test_ternary_executes_in_jaq() {
        // Test that generated jq actually works with jaq
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        // Test 1: Simple ternary
        let input = ".status == 200 ? \"OK\" : \"Error\"";
        let jq = ternary_to_jq(input);
        let response = json!({"status": 200});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("OK"));

        // Test 2: Simple ternary - false branch
        let response = json!({"status": 500});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("Error"));
    }

    #[test]
    fn test_ternary_nested_executes_in_jaq() {
        // Test nested ternary with jaq
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let jq = ternary_to_jq(input);
        
        // Test a=5 (positive but not big)
        let response = json!({"a": 5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("medium"));

        // Test a=15 (big)
        let response = json!({"a": 15});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("big"));

        // Test a=-5 (negative)
        let response = json!({"a": -5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("small"));
    }

    #[test]
    fn test_ternary_sequential_executes_in_jaq() {
        // Test sequential (right-associative) ternary with jaq
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".a > 0 ? .b > 0 ? \"both positive\" : \"a only\" : \"none\"";
        let jq = ternary_to_jq(input);
        
        // Test a=1, b=1 (both positive)
        let response = json!({"a": 1, "b": 1});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("both positive"));

        // Test a=1, b=-1 (a only)
        let response = json!({"a": 1, "b": -1});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("a only"));

        // Test a=-1, b=1 (none)
        let response = json!({"a": -1, "b": 1});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("none"));
    }

    #[test]
    fn test_ternary_with_parentheses_preserved() {
        // Verify parentheses are preserved correctly
        let input = ".a > 0 ? (.a > 10 ? \"big\" : \"medium\") : \"small\"";
        let result = ternary_to_jq(input);
        assert!(result.contains("(if .a > 10 then"));
        assert!(result.contains("end)"));
    }

    #[test]
    fn test_ternary_deep_nested_executes_in_jaq() {
        // Test 3-level nested ternary
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".a > 0 ? (.a > 10 ? (.a > 20 ? \"very big\" : \"big\") : \"medium\") : \"small\"";
        let jq = ternary_to_jq(input);
        
        // Test a=25 (very big)
        let response = json!({"a": 25});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("very big"));

        // Test a=15 (big)
        let response = json!({"a": 15});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("big"));

        // Test a=5 (medium)
        let response = json!({"a": 5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("medium"));

        // Test a=-5 (small)
        let response = json!({"a": -5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("small"));
    }

    #[test]
    fn test_ternary_with_jq_pipe_executes_in_jaq() {
        // Test ternary with JQ pipe expressions
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".items | length > 0 ? .items[0] : null";
        let jq = ternary_to_jq(input);
        
        // Test with non-empty array
        let response = json!({"items": ["first", "second"]});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("first"));

        // Test with empty array
        let response = json!({"items": []});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(null));
    }

    #[test]
    fn test_ternary_string_comparison_executes_in_jaq() {
        // Test ternary with string comparison
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".status == \"ok\" ? \"success\" : \"failure\"";
        let jq = ternary_to_jq(input);
        
        // Test matching string
        let response = json!({"status": "ok"});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("success"));

        // Test non-matching string
        let response = json!({"status": "error"});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("failure"));
    }

    #[test]
    fn test_ternary_numeric_comparison_executes_in_jaq() {
        // Test ternary with numeric comparisons
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".score >= 90 ? \"A\" : (.score >= 80 ? \"B\" : \"C\")";
        let jq = ternary_to_jq(input);
        
        // Test A grade
        let response = json!({"score": 95});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("A"));

        // Test B grade
        let response = json!({"score": 85});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("B"));

        // Test C grade
        let response = json!({"score": 70});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("C"));
    }

    #[test]
    fn test_ternary_nested_in_expression_executes_in_jaq() {
        // Test ternary used as part of larger expression
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        // Ternary in comparison
        let input = "(.a > 0 ? .a : 0) > 10";
        let jq = ternary_to_jq(input);
        
        // Test positive > 10
        let response = json!({"a": 15});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(true));

        // Test positive <= 10
        let response = json!({"a": 5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(false));

        // Test negative (uses 0)
        let response = json!({"a": -5});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!(false));
    }

    #[test]
    fn test_ternary_with_null_check_executes_in_jaq() {
        // Test ternary for null handling
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = ".value != null ? .value : \"default\"";
        let jq = ternary_to_jq(input);
        
        // Test with value
        let response = json!({"value": "custom"});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("custom"));

        // Test with null
        let response = json!({"value": null});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("default"));
    }

    #[test]
    fn test_ternary_complex_boolean_logic_executes_in_jaq() {
        // Test ternary with complex boolean logic
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        let input = "(.a > 0 and .b > 0) ? \"both positive\" : \"at least one non-positive\"";
        let jq = ternary_to_jq(input);
        
        // Test both positive
        let response = json!({"a": 1, "b": 2});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("both positive"));

        // Test one negative
        let response = json!({"a": -1, "b": 2});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("at least one non-positive"));
    }

    #[test]
    fn test_ternary_extract_pattern_executes_in_jaq() {
        // Test typical EXTRACT section pattern
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        // Simulate: token = .status == 200 ? .access_token : .refresh_token
        let input = ".status == 200 ? .access_token : .refresh_token";
        let jq = ternary_to_jq(input);
        
        // Test success response
        let response = json!({"status": 200, "access_token": "abc123", "refresh_token": "xyz789"});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("abc123"));

        // Test refresh response
        let response = json!({"status": 401, "access_token": "abc123", "refresh_token": "xyz789"});
        let results = engine.query(&jq, &response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], json!("xyz789"));
    }

    #[test]
    fn test_ternary_multiple_sequential_full_execution() {
        // Test full sequential ternary execution (right-associative)
        use crate::assert::engine::AssertionEngine;
        use serde_json::json;

        let engine = AssertionEngine::new();
        
        // .a > 0 ? .b > 0 ? "both" : "a only" : "none"
        let input = ".a > 0 ? .b > 0 ? \"both positive\" : \"a only\" : \"none\"";
        let jq = ternary_to_jq(input);
        
        // Verify generated JQ structure
        assert!(jq.starts_with("if .a > 0 then"));
        assert!(jq.contains("if .b > 0 then"));
        assert_eq!(jq.matches(" end").count(), 2);
        
        // Execute all 4 combinations
        let test_cases = [
            (json!({"a": 1, "b": 1}), json!("both positive")),
            (json!({"a": 1, "b": -1}), json!("a only")),
            (json!({"a": -1, "b": 1}), json!("none")),
            (json!({"a": -1, "b": -1}), json!("none")),
        ];
        
        for (response, expected) in test_cases {
            let results = engine.query(&jq, &response).unwrap();
            assert_eq!(results.len(), 1, "Failed for input: {:?}", response);
            assert_eq!(results[0], expected, "Failed for input: {:?}", response);
        }
    }
}
