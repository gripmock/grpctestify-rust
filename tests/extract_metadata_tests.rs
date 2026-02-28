// Tests for @header and @trailer plugins in EXTRACT section

use grpctestify::parser::parse_gctf_from_str;

#[test]
fn test_extract_with_header_plugin_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
result = .result
request_id = @header("x-request-id")
content_type = @header("content-type")

--- ASSERTS ---
.result == "ok"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "EXTRACT with @header should parse successfully"
    );
}

#[test]
fn test_extract_with_trailer_plugin_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
result = .result
status = @trailer("x-status")
checksum = @trailer("x-checksum")

--- ASSERTS ---
.result == "ok"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "EXTRACT with @trailer should parse successfully"
    );
}

#[test]
fn test_extract_with_header_and_trailer_syntax() {
    // Arrange
    let content = r#"
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}

--- EXTRACT ---
result = .result
request_id = @header("x-request-id")
status = @trailer("x-status")

--- ASSERTS ---
.result == "ok"
"#;

    // Act
    let result = parse_gctf_from_str(content, "test.gctf");

    // Assert
    assert!(
        result.is_ok(),
        "EXTRACT with @header and @trailer should parse successfully"
    );
}
