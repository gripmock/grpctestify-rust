#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
// String & Math operations tests via JQ expressions

use grpctestify::parser::ternary::process_extract_value;

#[test]
fn string_concatenation() {
    // String concatenation via JQ
    let input = ".first + \" \" + .last";
    let result = process_extract_value(input);

    // Should pass through as valid JQ
    assert!(result.contains("first"));
    assert!(result.contains("last"));
}

#[test]
fn string_uppercase() {
    // Uppercase via JQ
    let input = ".name | ascii_upcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_upcase"));
}

#[test]
fn string_lowercase() {
    // Lowercase via JQ
    let input = ".name | ascii_downcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_downcase"));
}

#[test]
fn string_length() {
    // String length via JQ
    let input = ".name | length";
    let result = process_extract_value(input);

    assert!(result.contains("length"));
}

#[test]
fn string_split() {
    // Split string via JQ
    let input = ".tags | split(\",\")";
    let result = process_extract_value(input);

    assert!(result.contains("split"));
}

#[test]
fn string_join() {
    // Join array via JQ
    let input = ".items | join(\", \")";
    let result = process_extract_value(input);

    assert!(result.contains("join"));
}

#[test]
fn string_substring() {
    // Substring via JQ slice
    let input = ".name[0:3]";
    let result = process_extract_value(input);

    assert!(result.contains("[0:3]"));
}

#[test]
fn string_replace() {
    // Replace via JQ gsub
    let input = ".text | gsub(\"old\"; \"new\")";
    let result = process_extract_value(input);

    assert!(result.contains("gsub"));
}

#[test]
fn math_addition() {
    // Addition via JQ
    let input = ".a + .b";
    let result = process_extract_value(input);

    assert!(result.contains(".a"));
    assert!(result.contains(".b"));
}

#[test]
fn math_subtraction() {
    // Subtraction via JQ
    let input = ".a - .b";
    let result = process_extract_value(input);

    assert!(result.contains(".a"));
    assert!(result.contains(".b"));
}

#[test]
fn math_multiplication() {
    // Multiplication via JQ
    let input = ".price * .quantity";
    let result = process_extract_value(input);

    assert!(result.contains("price"));
    assert!(result.contains("quantity"));
}

#[test]
fn math_division() {
    // Division via JQ
    let input = ".total / .count";
    let result = process_extract_value(input);

    assert!(result.contains("total"));
    assert!(result.contains("count"));
}

#[test]
fn math_modulo() {
    // Modulo via JQ
    let input = "5 % 3";
    let result = process_extract_value(input);

    assert!(result.contains("%"));
}

#[test]
fn math_min() {
    // Min via JQ
    let input = ".numbers | min";
    let result = process_extract_value(input);

    assert!(result.contains("min"));
}

#[test]
fn math_max() {
    // Max via JQ
    let input = ".numbers | max";
    let result = process_extract_value(input);

    assert!(result.contains("max"));
}

#[test]
fn math_sum() {
    // Sum via JQ add
    let input = ".numbers | add";
    let result = process_extract_value(input);

    assert!(result.contains("add"));
}

#[test]
fn math_round() {
    // Round via JQ
    let input = ".value | round";
    let result = process_extract_value(input);

    assert!(result.contains("round"));
}

#[test]
fn math_floor() {
    // Floor via JQ
    let input = ".value | floor";
    let result = process_extract_value(input);

    assert!(result.contains("floor"));
}

#[test]
fn math_ceil() {
    // Ceil via JQ
    let input = ".value | ceil";
    let result = process_extract_value(input);

    assert!(result.contains("ceil"));
}

#[test]
fn math_sort() {
    // Sort via JQ
    let input = ".numbers | sort";
    let result = process_extract_value(input);

    assert!(result.contains("sort"));
}

#[test]
fn conditional_string() {
    // Conditional with string
    let input = "if .name == \"Admin\" then \"Hello Admin\" else \"Hello \" + .name end";
    let result = process_extract_value(input);

    assert!(result.contains("if"));
    assert!(result.contains("then"));
    assert!(result.contains("else"));
    assert!(result.contains("end"));
}

#[test]
fn conditional_math() {
    // Conditional with math
    let input = "if .price > 100 then \"expensive\" else \"cheap\" end";
    let result = process_extract_value(input);

    assert!(result.contains("if"));
    assert!(result.contains("then"));
    assert!(result.contains("else"));
    assert!(result.contains("end"));
}

#[test]
fn combined_operations() {
    // Combined string operations
    let input = "(.first + \" \" + .last) | ascii_upcase";
    let result = process_extract_value(input);

    assert!(result.contains("ascii_upcase"));
}

#[test]
fn array_map() {
    // Map over array
    let input = ".items | map(.price * .qty)";
    let result = process_extract_value(input);

    assert!(result.contains("map"));
}

#[test]
fn array_filter() {
    // Filter array with select
    let input = ".items | map(select(.price > 50))";
    let result = process_extract_value(input);

    assert!(result.contains("select"));
}
