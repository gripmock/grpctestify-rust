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
fn test_fmt_preamble_sections_sorted_canonically() {
    let source = r#"--- ENDPOINT ---
svc/Method

--- OPTIONS ---
timeout: 10

--- ADDRESS ---
localhost:4770

--- TLS ---
ca_cert: /path/ca.crt

--- PROTO ---
files: service.proto

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;

    let formatted = format_with_serializer(source);
    let expected = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
svc/Method

--- TLS ---
ca_cert: /path/ca.crt

--- PROTO ---
files: service.proto

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;

    assert_eq!(formatted, expected);
}

#[test]
fn test_fmt_preamble_body_boundary_preserved() {
    let source = r#"--- ENDPOINT ---
svc/Method

--- REQUEST_HEADERS ---
authorization: Bearer token

--- REQUEST ---
{}

--- ASSERTS ---
@status() == "OK"

--- RESPONSE ---
{}
"#;

    let formatted = format_with_serializer(source);
    assert!(formatted.contains("--- REQUEST_HEADERS ---\nauthorization: Bearer token"));
    assert!(formatted.find("--- REQUEST ---") < formatted.find("--- ASSERTS ---"));
    assert!(formatted.find("--- ASSERTS ---") < formatted.find("--- RESPONSE ---"));
}

#[test]
fn test_fmt_bench_keys_canonical_order() {
    let source = r#"--- ENDPOINT ---
svc/Method

--- BENCH ---
duration: 30s
mode: fixed
concurrency: 16
profile: smoke
requests: 5000

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;

    let formatted = format_with_serializer(source);
    let bench_start = formatted.find("--- BENCH ---").unwrap();
    let bench_end = formatted[bench_start..].find("\n\n").unwrap() + bench_start;
    let bench_block = &formatted[bench_start..bench_end];

    let key_lines: Vec<&str> = bench_block
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .collect();

    let keys: Vec<&str> = key_lines
        .iter()
        .map(|l| l.split(':').next().unwrap().trim())
        .collect();
    assert_eq!(
        keys,
        vec!["mode", "profile", "concurrency", "requests", "duration"]
    );
}

#[test]
fn test_fmt_bench_after_meta_in_preamble() {
    let source = r#"--- ENDPOINT ---
svc/Method

--- BENCH ---
mode: fixed

--- META ---
name: test

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;

    let formatted = format_with_serializer(source);
    let meta_pos = formatted.find("--- META ---").unwrap();
    let bench_pos = formatted.find("--- BENCH ---").unwrap();
    let addr_pos = formatted.find("--- ENDPOINT ---").unwrap();
    let opts_pos = formatted.find("--- OPTIONS ---").unwrap();

    assert!(meta_pos < bench_pos, "META should come before BENCH");
    assert!(bench_pos < addr_pos, "BENCH should come before ENDPOINT");
    assert!(addr_pos < opts_pos, "ENDPOINT should come before OPTIONS");
}

#[test]
fn test_fmt_preserves_type_cast_in_asserts() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "price": 42
}

--- ASSERTS ---
.price:number >= 0
"#;

    let formatted = format_with_serializer(source);
    assert!(
        formatted.contains(".price:number >= 0"),
        "Type cast should be preserved in formatted output"
    );
    assert!(formatted.contains("--- ASSERTS ---"));
}

#[test]
fn test_fmt_preserves_type_cast_string_contains() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "name": "hello"
}

--- ASSERTS ---
.name:string contains "hello"
"#;

    let formatted = format_with_serializer(source);
    assert!(formatted.contains(".name:string contains \"hello\""));
}

#[test]
fn test_fmt_preserves_type_cast_plugin() {
    let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "items": [1, 2, 3]
}

--- ASSERTS ---
@len(.items):uint >= 0
"#;

    let formatted = format_with_serializer(source);
    assert!(formatted.contains("@len(.items):uint >= 0"));
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
