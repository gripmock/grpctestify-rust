use grpctestify::serialize_gctf;

fn format_with_serializer(content: &str) -> String {
    let doc = grpctestify::parser::parse_gctf_from_str(content, "test.gctf").unwrap();
    serialize_gctf(&doc)
}

#[test]
fn test_fmt_unary_strict() {
    let source = r#"--- ENDPOINT ---
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

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
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

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_multiple_pairs_strict() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 1
}

--- RESPONSE ---
{
  "result": "1"
}

--- REQUEST ---
{
  "id": 2
}

--- RESPONSE ---
{
  "result": "2"
}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 1
}

--- RESPONSE ---
{
  "result": "1"
}

--- REQUEST ---
{
  "id": 2
}

--- RESPONSE ---
{
  "result": "2"
}
"#;

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_headers_sorted() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Content-Type: application/json
Authorization: Bearer token123

--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Authorization: Bearer token123
Content-Type: application/json

--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_error_section_strict() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": "invalid"
}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid ID"
}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": "invalid"
}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid ID"
}
"#;

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_tls_sorted() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
insecure: false
ca_cert: /path/to/ca.crt

--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
ca_cert: /path/to/ca.crt
insecure: false

--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_idempotent() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted_once = format_with_serializer(source);
    let formatted_twice = format_with_serializer(&formatted_once);

    assert_eq!(formatted_once, formatted_twice);
}

#[test]
fn test_fmt_preserves_attribute_flag() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[skip]
--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    assert!(
        formatted.contains("#[skip]"),
        "fmt should preserve #[skip] attribute"
    );
}

#[test]
fn test_fmt_preserves_attribute_with_value() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[timeout(10)]
--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    assert!(
        formatted.contains("#[timeout(10)]"),
        "fmt should preserve #[timeout(10)]"
    );
}

#[test]
fn test_fmt_preserves_multiple_attributes() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[timeout(30)]
#[retry(3)]
--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    assert!(
        formatted.contains("#[timeout(30)]"),
        "fmt should preserve #[timeout(30)]"
    );
    assert!(
        formatted.contains("#[retry(3)]"),
        "fmt should preserve #[retry(3)]"
    );
}

#[test]
fn test_fmt_attributes_idempotent() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[skip]
--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted_once = format_with_serializer(source);
    let formatted_twice = format_with_serializer(&formatted_once);
    assert_eq!(formatted_once, formatted_twice);
}

#[test]
fn test_fmt_does_not_duplicate_attributes() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[timeout(10)]
--- REQUEST ---
{
  "id": 123
}

--- RESPONSE ---
{
  "result": "ok"
}
"#;

    let formatted = format_with_serializer(source);
    let count = formatted.matches("#[timeout(10)]").count();
    assert_eq!(count, 1, "fmt should not duplicate #[timeout] attribute");
}

#[test]
fn test_fmt_preserves_attributes_on_multiple_sections() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

#[timeout(30)]
--- REQUEST ---
{
  "id": 1
}

--- RESPONSE ---
{
  "result": "1"
}

#[retry(2)]
--- REQUEST ---
{
  "id": 2
}

--- RESPONSE ---
{
  "result": "2"
}
"#;

    let formatted = format_with_serializer(source);
    assert!(formatted.contains("#[timeout(30)]"));
    assert!(formatted.contains("#[retry(2)]"));
}

#[test]
fn test_fmt_keeps_attribute_on_request_not_endpoint() {
    let source = r#"--- ENDPOINT ---
extended.DesignService/GetThemeColor

#[name(test)]
--- REQUEST ---
{
  "themeId": "dark_theme"
}

--- RESPONSE ---
{
  "color": {
    "alpha": 0.8,
    "blue": 0.3,
    "green": 0.2,
    "red": 0.1
  }
}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ENDPOINT ---
extended.DesignService/GetThemeColor

#[name(test)]
--- REQUEST ---
{
  "themeId": "dark_theme"
}

--- RESPONSE ---
{
  "color": {
    "alpha": 0.8,
    "blue": 0.3,
    "green": 0.2,
    "red": 0.1
  }
}
"#;

    assert_eq!(formatted, expected);
}
