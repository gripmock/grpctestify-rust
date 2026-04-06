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
