// String & Math operations tests via JQ expressions

use grpctestify::parser::ternary::process_extract_value;

#[test]
fn test_string_concatenation() {
    // String concatenation via JQ
    let input = ".first + \" \" + .last";
    let result = process_extract_value(input);

    // Should pass through as valid JQ
    assert!(result.contains("first"));
    assert!(result.contains("last"));
}

#[test]
fn test_string_uppercase() {
    // Uppercase via JQ
    let input = ".name | ascii_upcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_upcase"));
}

#[test]
fn test_string_lowercase() {
    // Lowercase via JQ
    let input = ".name | ascii_downcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_downcase"));
}

#[test]
fn test_string_length() {
    // String length via JQ
    let input = ".name | length";
    let result = process_extract_value(input);

    assert!(result.contains("length"));
}

#[test]
fn test_string_split() {
    // Split string via JQ
    let input = ".tags | split(\",\")";
    let result = process_extract_value(input);

    assert!(result.contains("split"));
}

#[test]
fn test_string_join() {
    // Join array via JQ
    let input = ".items | join(\", \")";
    let result = process_extract_value(input);

    assert!(result.contains("join"));
}

#[test]
fn test_string_substring() {
    // Substring via JQ slice
    let input = ".name[0:3]";
    let result = process_extract_value(input);

    assert!(result.contains("[0:3]"));
}

#[test]
fn test_string_replace() {
    // Replace via JQ gsub
    let input = ".text | gsub(\"old\"; \"new\")";
    let result = process_extract_value(input);

    assert!(result.contains("gsub"));
}

#[test]
fn test_math_addition() {
    // Addition via JQ
    let input = ".a + .b";
    let result = process_extract_value(input);

    assert!(result.contains(".a"));
    assert!(result.contains(".b"));
}

#[test]
fn test_math_subtraction() {
    // Subtraction via JQ
    let input = ".a - .b";
    let result = process_extract_value(input);

    assert!(result.contains(".a"));
    assert!(result.contains(".b"));
}

#[test]
fn test_math_multiplication() {
    // Multiplication via JQ
    let input = ".price * .quantity";
    let result = process_extract_value(input);

    assert!(result.contains("price"));
    assert!(result.contains("quantity"));
}

#[test]
fn test_math_division() {
    // Division via JQ
    let input = ".total / .count";
    let result = process_extract_value(input);

    assert!(result.contains("total"));
    assert!(result.contains("count"));
}

#[test]
fn test_math_modulo() {
    // Modulo via JQ
    let input = "5 % 3";
    let result = process_extract_value(input);

    assert!(result.contains("%"));
}

#[test]
fn test_math_min() {
    // Min via JQ
    let input = ".numbers | min";
    let result = process_extract_value(input);

    assert!(result.contains("min"));
}

#[test]
fn test_math_max() {
    // Max via JQ
    let input = ".numbers | max";
    let result = process_extract_value(input);

    assert!(result.contains("max"));
}

#[test]
fn test_math_sum() {
    // Sum via JQ add
    let input = ".numbers | add";
    let result = process_extract_value(input);

    assert!(result.contains("add"));
}

#[test]
fn test_math_round() {
    // Round via JQ
    let input = ".value | round";
    let result = process_extract_value(input);

    assert!(result.contains("round"));
}

#[test]
fn test_math_floor() {
    // Floor via JQ
    let input = ".value | floor";
    let result = process_extract_value(input);

    assert!(result.contains("floor"));
}

#[test]
fn test_math_ceil() {
    // Ceil via JQ
    let input = ".value | ceil";
    let result = process_extract_value(input);

    assert!(result.contains("ceil"));
}

#[test]
fn test_math_sort() {
    // Sort via JQ
    let input = ".numbers | sort";
    let result = process_extract_value(input);

    assert!(result.contains("sort"));
}

#[test]
fn test_conditional_string() {
    // Conditional with string
    let input = "if .name == \"Admin\" then \"Hello Admin\" else \"Hello \" + .name end";
    let result = process_extract_value(input);

    assert!(result.contains("if"));
    assert!(result.contains("then"));
    assert!(result.contains("else"));
    assert!(result.contains("end"));
}

#[test]
fn test_conditional_math() {
    // Conditional with math
    let input = "if .price > 100 then \"expensive\" else \"cheap\" end";
    let result = process_extract_value(input);

    assert!(result.contains("if"));
    assert!(result.contains("then"));
    assert!(result.contains("else"));
    assert!(result.contains("end"));
}

#[test]
fn test_combined_operations() {
    // Combined string operations
    let input = "(.first + \" \" + .last) | ascii_upcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_upcase"));
}

#[test]
fn test_array_map() {
    // Map over array
    let input = ".items | map(.price * .qty)";
    let result = process_extract_value(input);

    assert!(result.contains("map"));
}

#[test]
fn test_array_filter() {
    // Filter array with select
    let input = ".items | map(select(.price > 50))";
    let result = process_extract_value(input);

    assert!(result.contains("select"));
}
