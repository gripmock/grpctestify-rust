// FMT (format) command tests

use grpctestify::parser::parse_gctf;
use grpctestify::serialize_gctf;
use std::path::Path;

#[test]
fn test_fmt_preserves_content() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123,
  "name": "test"
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("test.Service/Method"),
        "Endpoint should be preserved"
    );
    assert!(
        formatted.contains("\"id\": 123"),
        "Request content should be preserved"
    );
    assert!(
        formatted.contains("\"result\": \"ok\""),
        "Response content should be preserved"
    );
}

#[test]
fn test_fmt_normalizes_section_headers() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- ENDPOINT ---"),
        "ENDPOINT header should be present"
    );
    assert!(
        formatted.contains("--- REQUEST ---"),
        "REQUEST header should be present"
    );
    assert!(
        formatted.contains("--- RESPONSE ---"),
        "RESPONSE header should be present"
    );
}

#[test]
fn test_fmt_preserves_comments() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

// This is a comment
--- REQUEST ---
{
  "id": 123 // inline comment
}

--- RESPONSE ---
{"result": "ok"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("// This is a comment"),
        "Section comments should be preserved"
    );
}

#[test]
fn test_fmt_from_example_file() {
    // Arrange
    let path = "examples/basic/unary.gctf";

    // Act
    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- ENDPOINT ---"),
        "ENDPOINT section should be present"
    );
    assert!(
        formatted.contains("--- REQUEST ---"),
        "REQUEST section should be present"
    );
    assert!(
        formatted.contains("--- RESPONSE ---"),
        "RESPONSE section should be present"
    );

    let re_parsed = grpctestify::parser::parse_gctf_from_str(&formatted, "formatted.gctf");
    assert!(re_parsed.is_ok(), "Formatted output should be valid GCTF");
}

#[test]
fn test_fmt_multiple_sections() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 1}

--- RESPONSE ---
{"result": "1"}

--- REQUEST ---
{"id": 2}

--- RESPONSE ---
{"result": "2"}

--- REQUEST ---
{"id": 3}

--- RESPONSE ---
{"result": "3"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("\"id\": 1"),
        "First request should be preserved"
    );
    assert!(
        formatted.contains("\"id\": 2"),
        "Second request should be preserved"
    );
    assert!(
        formatted.contains("\"id\": 3"),
        "Third request should be preserved"
    );

    let request_count = formatted.matches("--- REQUEST ---").count();
    let response_count = formatted.matches("--- RESPONSE ---").count();
    assert_eq!(request_count, 3, "Expected 3 REQUEST sections");
    assert_eq!(response_count, 3, "Expected 3 RESPONSE sections");
}

#[test]
fn test_fmt_extract_asserts() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"id": 123, "token": "abc"}

--- EXTRACT ---
id = .id
token = .token

--- ASSERTS ---
.id == 123
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- EXTRACT ---"),
        "EXTRACT section should be present"
    );
    assert!(
        formatted.contains("--- ASSERTS ---"),
        "ASSERTS section should be present"
    );

    let re_parsed = grpctestify::parser::parse_gctf_from_str(&formatted, "formatted.gctf");
    assert!(re_parsed.is_ok(), "Formatted output should be valid GCTF");
}

#[test]
fn test_fmt_inline_options() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"id": 123}

--- ASSERTS ---
.id == 123
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- RESPONSE ---"),
        "RESPONSE section should be present"
    );
    assert!(
        formatted.contains("--- ASSERTS ---"),
        "ASSERTS section should be present"
    );
}

#[test]
fn test_fmt_headers() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Authorization: Bearer token123
Content-Type: application/json

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- REQUEST_HEADERS ---"),
        "REQUEST_HEADERS section should be present"
    );
    assert!(
        formatted.contains("Authorization:"),
        "Authorization header should be preserved"
    );
    assert!(
        formatted.contains("Content-Type:"),
        "Content-Type header should be preserved"
    );
}

#[test]
fn test_fmt_error_section() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": "invalid"}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid ID"
}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- ERROR ---"),
        "ERROR section should be present"
    );
    assert!(
        formatted.contains("\"code\": 3"),
        "Error code should be preserved"
    );
    assert!(
        formatted.contains("Invalid ID"),
        "Error message should be preserved"
    );
}

#[test]
fn test_fmt_tls_section() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
ca_cert: /path/to/ca.crt
insecure: false

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}
"#;

    // Act
    let doc = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted = serialize_gctf(&doc);

    // Assert
    assert!(
        formatted.contains("--- TLS ---"),
        "TLS section should be present"
    );
    assert!(
        formatted.contains("ca_cert:"),
        "ca_cert should be preserved"
    );
    assert!(
        formatted.contains("insecure:"),
        "insecure should be preserved"
    );
}

#[test]
fn test_fmt_idempotency() {
    // Arrange
    let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": 123}

--- RESPONSE ---
{"result": "ok"}
"#;

    // Act
    let doc1 = parse_gctf_from_str(content, "test.gctf").unwrap();
    let formatted1 = serialize_gctf(&doc1);

    let doc2 = parse_gctf_from_str(&formatted1, "formatted1.gctf").unwrap();
    let formatted2 = serialize_gctf(&doc2);

    // Assert
    assert_eq!(formatted1, formatted2, "Formatting should be idempotent");
}

fn parse_gctf_from_str(
    content: &str,
    path: &str,
) -> Result<grpctestify::parser::GctfDocument, anyhow::Error> {
    grpctestify::parser::parse_gctf_from_str(content, path)
}
