#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
use grpctestify::serialize_gctf;

#[path = "support/mod.rs"]
mod support;

fn format_with_serializer(content: &str) -> String {
    let doc = grpctestify::parser::parse_gctf_from_str(content, "test.gctf").unwrap();
    serialize_gctf(&doc)
}

#[test]
fn fmt_unary_strict() {
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
fn fmt_preamble_sections_sorted_canonically() {
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
fn fmt_preamble_body_boundary_preserved() {
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
fn fmt_bench_keys_canonical_order() {
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
fn fmt_bench_after_meta_in_preamble() {
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
fn fmt_preserves_type_cast_in_asserts() {
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
fn fmt_preserves_type_cast_string_contains() {
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
fn fmt_preserves_type_cast_plugin() {
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
fn fmt_keeps_attribute_on_request_not_endpoint() {
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

#[test]
fn fmt_preserves_escapes_in_assertion_string_literals() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("esc.gctf");
    let asserts = [
        r#"!(.p == "a\\b")"#,
        r#"!(.q == "tab\there")"#,
        r#"!(.r == "line\nbreak")"#,
        r#".m == "say \"hi\"""#,
    ];
    let content = format!(
        "--- ENDPOINT ---\nsvc.S/M\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{{ \"p\": \"x\" }}\n\n--- ASSERTS ---\n{}\n",
        asserts.join("\n")
    );
    std::fs::write(&file, &content).unwrap();

    let output = support::cli_command()
        .args(["fmt", "--write", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The literals must survive byte-for-byte. The surrounding expression may
    // legitimately be rewritten (`!(x == y)` folding to `x != y`), so this pins
    // the payloads, not the layout.
    let after = std::fs::read_to_string(&file).unwrap();
    let literals = [
        r#""a\\b""#,
        r#""tab\there""#,
        r#""line\nbreak""#,
        r#""say \"hi\"""#,
    ];
    for literal in literals {
        assert!(
            after.contains(literal),
            "fmt altered a string literal: {literal} missing from\n{after}"
        );
    }
}

#[test]
fn fmt_keeps_what_an_assertion_means_unless_asked_to_optimize() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain.gctf");
    let source = "--- ENDPOINT ---\nsvc.S/M\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{ \"p\": \"x\" }\n\n--- ASSERTS ---\n!(.plain == \"abc\")\nif true then .a else .b end\nnot (.x == 1 or .y == 2)\n@is_uuid(.id) == true\n";
    std::fs::write(&file, source).unwrap();

    let output = support::cli_command()
        .args(["fmt", "--write", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt");
    assert!(output.status.success());

    let after = std::fs::read_to_string(&file).unwrap();
    for kept in [
        r#"!(.plain == "abc")"#,
        "if true then .a else .b end",
        "not (.x == 1 or .y == 2)",
        "@is_uuid(.id) == true",
    ] {
        assert!(
            after.contains(kept),
            "fmt rewrote the meaning of {kept}:\n{after}"
        );
    }

    let output = support::cli_command()
        .args(["fmt", "-O", "safe", "--write", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt");
    assert!(output.status.success());

    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.contains(r#".plain != "abc""#),
        "the negation rule still applies when the optimizer is asked for:\n{after}"
    );
    assert!(
        !after.contains("if true then"),
        "the dead branch goes when the optimizer is asked for:\n{after}"
    );
}

#[test]
fn fmt_groups_digits_in_assertions_but_not_in_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("g.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\nsvc.S/M\n\n--- REQUEST ---\n{ \"n\": 6000000 }\n\n--- RESPONSE ---\n{ \"product\": 1000000, \"ratio\": 1.500000 }\n\n--- ASSERTS ---\n.count == 1000000\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["fmt", "--write", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt");
    assert!(output.status.success());
    let after = std::fs::read_to_string(&file).unwrap();

    assert!(
        after.contains("\"n\": 6000000") && after.contains("\"product\": 1000000"),
        "payload digits must stay ungrouped:\n{after}"
    );
    assert!(
        after.contains(".count == 1_000_000"),
        "assertion digits are still grouped:\n{after}"
    );
    assert!(
        after.contains("\"ratio\": 1.5"),
        "fraction trimming still applies inside payloads:\n{after}"
    );

    // The payload must be readable by a plain JSON parser.
    let body = after
        .split("--- RESPONSE ---\n")
        .nth(1)
        .and_then(|s| s.split("\n\n").next())
        .expect("response body");
    serde_json::from_str::<serde_json::Value>(body)
        .unwrap_or_else(|e| panic!("formatted payload is not valid JSON: {e}\n{body}"));
}
