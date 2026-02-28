// Tests for ternary operator in EXTRACT section

use grpctestify::parser::{parse_gctf_from_str, ternary::ternary_to_jq};

#[test]
fn test_extract_ternary_basic() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"status": 200}

--- EXTRACT ---
status_label = .status == 200 ? "OK" : "Error"

--- ASSERTS ---
.status_label == "OK"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with ternary should parse successfully"
    );
}

#[test]
fn test_extract_ternary_with_jq() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"items": [1, 2, 3]}

--- RESPONSE ---
{"items": [1, 2, 3]}

--- EXTRACT ---
has_items = (.items | length) > 0 ? "yes" : "no"

--- ASSERTS ---
.has_items == "yes"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with JQ in ternary should parse successfully"
    );
}

#[test]
fn test_extract_ternary_nested() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"count": 5}

--- RESPONSE ---
{"count": 5}

--- EXTRACT ---
size = .count == 0 ? "empty" : (.count > 10 ? "large" : "small")

--- ASSERTS ---
.size == "small"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with nested ternary should parse successfully"
    );
}

#[test]
fn test_extract_ternary_string_comparison() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"message": "OK"}

--- RESPONSE ---
{"message": "OK"}

--- EXTRACT ---
result = .message == "OK" ? "Success" : "Failed"

--- ASSERTS ---
.result == "Success"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with string comparison in ternary should parse successfully"
    );
}

#[test]
fn test_extract_mixed_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"status": 200, "code": "OK"}

--- RESPONSE ---
{"status": 200, "code": "OK"}

--- EXTRACT ---
# Ternary syntax
status_ternary = .status == 200 ? "OK" : "Error"

# JQ native syntax
status_jq = if .status == 200 then "OK" else "Error" end

--- ASSERTS ---
.status_ternary == "OK"
.status_jq == "OK"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with mixed syntax should parse successfully"
    );
}

#[test]
fn test_ternary_conversion_function() {
    // Arrange
    let input = ".status == 200 ? \"OK\" : \"Error\"";
    let expected = "if .status == 200 then \"OK\" else \"Error\" end";

    // Act
    let result = ternary_to_jq(input);

    // Assert
    assert_eq!(result, expected);
}

#[test]
fn test_ternary_with_header_plugin() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
request_id = @header("x-request-id") != null ? @header("x-request-id") : "unknown"

--- ASSERTS ---
@len({{ request_id }}) > 0
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with ternary and @header should parse successfully"
    );
}

#[test]
fn test_ternary_with_trailer_plugin() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
cache_status = @trailer("x-cache") == "HIT" ? "cached" : "fresh"

--- ASSERTS ---
@len({{ cache_status }}) > 0
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with ternary and @trailer should parse successfully"
    );
}
