// Integration tests for ternary AST in EXTRACT section

use grpctestify::parser::{
    parse_gctf_from_str,
    ternary_ast::{ExtractValue, ExtractVar},
};

// ============================================================================
// ExtractValue AST Tests
// ============================================================================

#[test]
fn test_extract_value_ast_simple_path() {
    // Arrange
    let input = ".user.id";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Simple(_)));
    assert_eq!(value.to_jq(), ".user.id");
}

#[test]
fn test_extract_value_ast_jq_pipe() {
    // Arrange
    let input = ".items | length";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::JqExpr(_)));
    assert_eq!(value.to_jq(), ".items | length");
}

#[test]
fn test_extract_value_ast_ternary_basic() {
    // Arrange
    let input = ".status == 200 ? \"OK\" : \"Error\"";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Ternary(_)));
    assert!(value.to_jq().starts_with("if"));
    assert!(value.to_jq().ends_with("end"));
    assert_eq!(
        value.to_jq(),
        "if .status == 200 then \"OK\" else \"Error\" end"
    );
}

#[test]
fn test_extract_value_ast_ternary_with_parens() {
    // Arrange
    let input = "(.items | length) > 0 ? \"yes\" : \"no\"";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Ternary(_)));
    let jq = value.to_jq();
    assert!(jq.contains("if"));
    assert!(jq.contains("then"));
    assert!(jq.contains("else"));
}

#[test]
fn test_extract_value_ast_ternary_nested() {
    // Arrange
    let input = ".a > 0 ? (.a > 10 ? \"big\" : \"small\") : \"zero\"";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Ternary(_)));
    let jq = value.to_jq();
    assert!(jq.contains("if .a > 0 then"));
    assert!(jq.contains("if .a > 10 then"));
}

#[test]
fn test_extract_value_ast_ternary_with_header() {
    // Arrange
    let input = "@header(\"x-request-id\") != null ? @header(\"x-request-id\") : \"unknown\"";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Ternary(_)));
}

#[test]
fn test_extract_value_ast_ternary_with_trailer() {
    // Arrange
    let input = "@trailer(\"x-cache\") == \"HIT\" ? \"cached\" : \"fresh\"";

    // Act
    let value = ExtractValue::parse(input);

    // Assert
    assert!(matches!(value, ExtractValue::Ternary(_)));
}

// ============================================================================
// ExtractVar AST Tests
// ============================================================================

#[test]
fn test_extract_var_ast_simple() {
    // Arrange
    let input = "token = .access_token";

    // Act
    let var = ExtractVar::parse(input).unwrap();

    // Assert
    assert_eq!(var.name, "token");
    assert!(matches!(var.value, ExtractValue::Simple(_)));
    assert_eq!(var.to_jq(), "token = .access_token");
}

#[test]
fn test_extract_var_ast_jq() {
    // Arrange
    let input = "count = .items | length";

    // Act
    let var = ExtractVar::parse(input).unwrap();

    // Assert
    assert_eq!(var.name, "count");
    assert!(matches!(var.value, ExtractValue::JqExpr(_)));
}

#[test]
fn test_extract_var_ast_ternary() {
    // Arrange
    let input = "status = .status == 200 ? \"OK\" : \"Error\"";

    // Act
    let var = ExtractVar::parse(input).unwrap();

    // Assert
    assert_eq!(var.name, "status");
    assert!(matches!(var.value, ExtractValue::Ternary(_)));
    assert!(var.to_jq().contains("if"));
    assert!(var.to_jq().contains("then"));
    assert!(var.to_jq().contains("else"));
}

#[test]
fn test_extract_var_ast_skip_comment() {
    // Arrange
    let input = "# this is a comment";

    // Act
    let var = ExtractVar::parse(input);

    // Assert
    assert!(var.is_none());
}

#[test]
fn test_extract_var_ast_skip_empty() {
    // Arrange
    let input = "";

    // Act
    let var = ExtractVar::parse(input);

    // Assert
    assert!(var.is_none());
}

#[test]
fn test_extract_var_ast_skip_whitespace() {
    // Arrange
    let input = "   ";

    // Act
    let var = ExtractVar::parse(input);

    // Assert
    assert!(var.is_none());
}

#[test]
fn test_extract_var_ast_with_spaces() {
    // Arrange
    let input = "  token  =  .access_token  ";

    // Act
    let var = ExtractVar::parse(input).unwrap();

    // Assert
    assert_eq!(var.name, "token");
    assert_eq!(var.value.to_jq(), ".access_token");
}

// ============================================================================
// Ternary to JQ Conversion Tests
// ============================================================================

#[test]
fn test_ternary_conversion_to_jq() {
    // Test basic ternary converts to if-then-else
    let value = ExtractValue::parse(".status == 200 ? \"OK\" : \"Error\"");
    assert!(matches!(value, ExtractValue::Ternary(_)));
    assert_eq!(
        value.to_jq(),
        "if .status == 200 then \"OK\" else \"Error\" end"
    );
}

#[test]
fn test_ternary_conversion_nested() {
    // Test nested ternary converts recursively
    let value = ExtractValue::parse(".a > 0 ? (.a > 10 ? \"big\" : \"small\") : \"zero\"");
    let jq = value.to_jq();
    assert!(jq.starts_with("if .a > 0 then"));
    assert!(jq.contains("if .a > 10 then"));
    assert!(jq.ends_with("end"));
}

#[test]
fn test_ternary_conversion_with_plugins() {
    // Test ternary with plugin calls
    let value = ExtractValue::parse("@header(\"x\") != null ? @header(\"x\") : \"default\"");
    let jq = value.to_jq();
    assert!(jq.starts_with("if @header(\"x\") != null then"));
}

// ============================================================================
// Full GCTF Document Tests
// ============================================================================

#[test]
fn test_full_gctf_with_ternary() {
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
fn test_full_gctf_with_multiple_ternary() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"status": 200, "count": 5}

--- EXTRACT ---
status_label = .status == 200 ? "OK" : "Error"
has_data = .count > 0 ? "yes" : "no"
size = .count > 10 ? "large" : "small"

--- ASSERTS ---
.status_label == "OK"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with multiple ternary should parse successfully"
    );
}

#[test]
fn test_full_gctf_mixed_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"status": 200}

--- EXTRACT ---
# Ternary syntax
status_ternary = .status == 200 ? "OK" : "Error"

# JQ native syntax
status_jq = if .status == 200 then "OK" else "Error" end

# Simple path
simple = .status

--- ASSERTS ---
.status_ternary == "OK"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "GCTF with mixed syntax should parse successfully"
    );
}
