#![cfg(not(miri))]

use std::process::{Command, Output};

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

fn fixture_path(rel: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

fn run_cli(args: &[&str]) -> Output {
    let binary = get_binary();
    let runner = std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER")
        .ok()
        .or_else(|| std::env::var("CROSS_RUNNER").ok());

    let mut cmd = if let Some(runner) = runner {
        let mut parts = runner.split_whitespace();
        let program = parts.next().expect("Runner must not be empty");
        let mut command = Command::new(program);
        command.args(parts);
        command.arg(&binary);
        command
    } else {
        Command::new(&binary)
    };

    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("Failed to execute CLI command")
}

fn parse_json_stdout(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "CLI failed with status {:?}\nstderr:\n{}\nstdout:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "Invalid JSON output: {e}\nstderr:\n{}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn parse_json_stdout_any_status(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "Invalid JSON output: {e}\nstderr:\n{}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn inspect_contract_view(json: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "has_file": json.get("file").is_some(),
        "parse_time_is_number": json.get("parse_time_ms").and_then(|v| v.as_f64()).is_some(),
        "validation_time_is_number": json.get("validation_time_ms").and_then(|v| v.as_f64()).is_some(),
        "sections": json["ast"]["sections"],
        "diagnostics": json["diagnostics"],
        "semantic_diagnostics": json["semantic_diagnostics"],
        "optimization_hints": json["optimization_hints"],
        "inferred_rpc_mode": json["inferred_rpc_mode"],
    })
}

fn explain_contract_view(json: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "has_semantic_plan": json.get("semantic_plan").is_some(),
        "has_optimization_trace": json.get("optimization_trace").is_some(),
        "has_optimized_plan": json.get("optimized_plan").is_some(),
        "has_execution_plan": json.get("execution_plan").is_some(),
        "optimization_trace": json["optimization_trace"],
        "semantic_summary": json["semantic_plan"]["summary"],
        "optimized_summary": json["optimized_plan"]["summary"],
        "execution_summary": json["execution_plan"]["summary"],
        "semantic_target": json["semantic_plan"]["target"],
        "optimized_target": json["optimized_plan"]["target"],
        "execution_target": json["execution_plan"]["target"],
    })
}

#[test]
fn test_fmt_preserves_comments_and_json5_content() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("comments.gctf");
    let content = r#"# file header comment
--- ADDRESS ---
localhost:50051

# endpoint comment
--- ENDPOINT ---
example.Service/Call

--- REQUEST ---
# request comment
{ "a": 1, // inline comment
  "b": 2 }

--- ASSERTS ---
# assert explanation
.a == 1
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("// file header comment"));
    assert!(updated.contains("// endpoint comment"));
    assert!(updated.contains("// assert explanation"));
    assert!(updated.contains("\"a\": 1"));
    assert!(updated.contains("\"b\": 2"));
}

#[test]
fn test_fmt_write_rewrites_json_inside_sections() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("rewrite-json.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name":"World","meta":{"id":1}}

--- RESPONSE partial=true ---
{"message":"Hello","ok":true}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("\"name\": \"World\""));
    assert!(updated.contains("\"meta\": {"));
    assert!(updated.contains("\"id\": 1"));
    assert!(updated.contains("--- RESPONSE partial=true ---"));
    assert!(updated.contains("\"ok\": true"));
}

#[test]
fn test_fmt_write_preserves_inline_json_comments() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("preserve-json-comments.gctf");
    let content = r#"--- ENDPOINT ---
scalar.FileService/UploadFile

--- REQUEST ---
{
  "content": "aGVsbG8="  # "hello" in Base64
}

--- RESPONSE ---
{
  "checksum": "5d41402abc4b2a76b9719d911017c592"  # MD5 hash of "hello"
}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("# \"hello\" in Base64"));
    assert!(updated.contains("# MD5 hash of \"hello\""));
}

#[test]
fn test_fmt_write_preserves_blank_line_between_sections() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("preserve-section-spacing.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast

--- REQUEST ---
{}

--- RESPONSE ---
{"condition":"Sunny","date":{"day":5,"month":10,"year":2023},"temperatureC":22.5}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("--- REQUEST ---\n{}\n\n--- RESPONSE ---"));
    assert!(updated.contains("\"temperatureC\": 22.5"));
}

#[test]
fn test_fmt_write_inserts_blank_line_between_adjacent_sections() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("insert-section-spacing.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast
--- REQUEST ---
{}
--- RESPONSE ---
{"condition":"Sunny"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("GetCurrentForecast\n\n--- REQUEST ---"));
    assert!(updated.contains("--- REQUEST ---\n{}\n\n--- RESPONSE ---"));
}

#[test]
fn test_fmt_write_preserves_response_trailing_hash_comment() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("response-trailing-hash-comment.gctf");
    let content = r#"--- ENDPOINT ---
scalar.FileService/UploadFile

--- REQUEST ---
{
    "content": "aGVsbG8="  # "hello" in Base64
}

--- RESPONSE ---
{
    "checksum": "5d41402abc4b2a76b9719d911017c592"}  # MD5 hash of "hello"
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("# \"hello\" in Base64"));
    assert!(updated.contains("# MD5 hash of \"hello\""));
    assert!(updated.contains("--- RESPONSE ---"));
}

#[test]
fn test_fmt_write_preserves_inline_block_comment_in_json() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("preserve-inline-block-comment.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast

--- REQUEST ---
{}

--- RESPONSE ---
{"condition":"Sunny","date":{"day":5 /** five */,"month":10,"year":2023},"temperatureC":22.5}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated =
        std::fs::read_to_string(&file).expect("failed to read rewritten gctf file content");
    assert!(updated.contains("/** five */"));
    assert!(updated.contains("\"day\": 5 /** five */"));
    assert!(updated.contains("\"temperatureC\": 22.5"));
}

#[test]
fn test_fmt_write_formats_json5_to_canonical_json() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("json5-canonical.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{
  name: 'World',
  meta: { id: 1, },
}

--- RESPONSE ---
{ message: 'Hello World' }
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("\"name\": \"World\""));
    assert!(updated.contains("\"meta\": {"));
    assert!(updated.contains("\"id\": 1"));
    assert!(updated.contains("\"message\": \"Hello World\""));
    assert!(!updated.contains("'World'"));
    assert!(!updated.contains("name:"));
}

#[test]
fn test_fmt_write_formats_response_jsonlines() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("jsonlines-response.gctf");
    let content = r#"--- ENDPOINT ---
track.StreamService/Read

--- REQUEST ---
{"id":"abc"}

--- RESPONSE ---
{"seq":1,"ok":true}
{"seq":2,"ok":true}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("\"seq\": 1"));
    assert!(updated.contains("\"seq\": 2"));
    assert!(updated.contains("\"ok\": true"));
}

#[test]
fn test_fmt_write_formats_jsonc_and_preserves_line_comments() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("jsonc-comments.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast

--- REQUEST ---
{}

--- RESPONSE ---
{
  // weather payload
  "condition":"Sunny",
  "temperatureC":22.5
}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("// weather payload"));
    assert!(updated.contains("\"condition\": \"Sunny\""));
    assert!(updated.contains("\"temperatureC\": 22.5"));
}

#[test]
fn test_fmt_write_preserves_hash_inside_string() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("hash-in-string.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast

--- REQUEST ---
{}

--- RESPONSE ---
{"station":"MS#00006","temperatureC":22.5}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("\"station\": \"MS#00006\""));
    assert!(updated.contains("\"temperatureC\": 22.5"));
}

#[test]
fn test_fmt_write_preserves_multiline_block_comment_in_json() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("multiline-block-comment.gctf");
    let content = r#"--- ENDPOINT ---
weather.WeatherService/GetCurrentForecast

--- REQUEST ---
{}

--- RESPONSE ---
{
  "date": {
    "day": 5,
    /* this is
       a multiline
       block comment */
    "month": 10,
    "year": 2023
  }
}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("/* this is"));
    assert!(updated.contains("a multiline"));
    assert!(updated.contains("block comment */"));
    assert!(updated.contains("\"month\": 10"));
}

#[test]
fn test_fmt_check_mode_reports_each_file_needing_format() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file1 = dir.path().join("needs-format-1.gctf");
    let file2 = dir.path().join("needs-format-2.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name":"World"}

--- RESPONSE ---
{"message":"Hello World"}
"#;
    std::fs::write(&file1, content).expect("failed to write temp gctf file 1");
    std::fs::write(&file2, content).expect("failed to write temp gctf file 2");

    let path1 = file1.to_string_lossy().into_owned();
    let path2 = file2.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", &path1, &path2]);

    assert!(
        !output.status.success(),
        "fmt check mode must fail when formatting is needed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("needs-format-1.gctf:1: [FORMAT_NEEDED]"));
    assert!(stdout.contains("needs-format-2.gctf:1: [FORMAT_NEEDED]"));
}

#[test]
fn test_fmt_check_mode_ignores_crlf_only_diff() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("crlf-formatted.gctf");
    let content = "--- ENDPOINT ---\r\nexample.v1.Greeter/SayHello\r\n\r\n--- REQUEST ---\r\n{\r\n  \"name\": \"World\"\r\n}\r\n\r\n--- RESPONSE ---\r\n{\r\n  \"message\": \"Hello World!\"\r\n}\r\n";
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", &path]);

    assert!(
        output.status.success(),
        "fmt check mode should ignore CRLF-only diff\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_fmt_applies_optimizer_by_default() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("fmt-opt-default.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}

--- ASSERTS ---
@has_header("x-request-id") == true
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", "-w", &path]);
    assert!(
        output.status.success(),
        "fmt -w command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let updated = std::fs::read_to_string(&file).expect("failed to read rewritten gctf file");
    assert!(updated.contains("@has_header(\"x-request-id\")"));
    assert!(!updated.contains("@has_header(\"x-request-id\") == true"));
}

#[test]
fn test_fmt_rejects_removed_optimize_flag() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["fmt", "-o", &file]);

    assert!(
        !output.status.success(),
        "fmt -o must fail after optimize flag removal\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '-o'")
            || stderr.contains("unexpected argument '--optimize'"),
        "expected clap error about removed -o flag, got:\n{}",
        stderr
    );
}

#[test]
fn test_run_subcommand_uses_dry_run_flag() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("dry-run.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["run", &path, "--dry-run"]);

    assert!(
        output.status.success(),
        "run --dry-run command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Dry-Run Preview"),
        "expected dry-run preview output, got:\n{}",
        stdout
    );
}

#[test]
fn test_explain_shows_options_section_details() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("explain-options.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- OPTIONS ---
timeout: 5
retry: 2

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["explain", &path]);

    assert!(
        output.status.success(),
        "explain command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Step 2: OPTIONS"));
    assert!(stdout.contains("Runtime overrides"));
    assert!(stdout.contains("timeout: 5"));
    assert!(stdout.contains("retry: 2"));
}

#[test]
fn test_inspect_shows_options_overrides_in_flow() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("inspect-options.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- OPTIONS ---
timeout: 5
retry: 2

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["inspect", &path]);

    assert!(
        output.status.success(),
        "inspect command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OPTIONS Overrides:"));
    assert!(stdout.contains("- retry: 2"));
    assert!(stdout.contains("- timeout: 5"));
}

#[test]
fn test_check_valid_file_json_output() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["check", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    assert!(json.get("diagnostics").is_some());
    assert!(json.get("summary").is_some());
    assert_eq!(json["summary"]["total_files"], 1);
}

#[test]
fn test_check_missing_file_json_output() {
    let file = fixture_path("tests/data/gctf/nonexistent.gctf");
    let output = run_cli(&["check", &file, "--format", "json"]);
    assert!(!output.status.success(), "missing file should fail check");
    let json = parse_json_stdout_any_status(&output);

    assert!(!json["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(json["diagnostics"][0]["code"], "FILE_NOT_FOUND");
}

#[test]
fn test_inspect_valid_file_json_output() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["inspect", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    assert!(json.get("file").is_some());
    assert!(json.get("ast").is_some());
    assert!(json.get("diagnostics").is_some());
    assert!(json.get("semantic_diagnostics").is_some());
    assert!(json.get("optimization_hints").is_some());
    assert!(json.get("inferred_rpc_mode").is_some());
}

#[test]
fn test_explain_valid_file_json_output() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["explain", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    assert!(json.get("semantic_plan").is_some());
    assert!(json.get("optimization_trace").is_some());
    assert!(json.get("optimized_plan").is_some());
    assert!(json.get("execution_plan").is_some());
}

#[test]
fn test_inspect_json_golden_contract() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["inspect", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    let actual = inspect_contract_view(&json);
    let expected = serde_json::json!({
        "has_file": true,
        "parse_time_is_number": true,
        "validation_time_is_number": true,
        "sections": [
            {
                "section_type": "ENDPOINT",
                "start_line": 0,
                "end_line": 2,
                "content_kind": "single"
            },
            {
                "section_type": "REQUEST",
                "start_line": 3,
                "end_line": 7,
                "content_kind": "json"
            },
            {
                "section_type": "RESPONSE",
                "start_line": 8,
                "end_line": 11,
                "content_kind": "json"
            }
        ],
        "diagnostics": [],
        "semantic_diagnostics": [],
        "optimization_hints": [],
        "inferred_rpc_mode": "Unary"
    });

    assert_eq!(actual, expected);
}

#[test]
fn test_explain_json_golden_contract() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["explain", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    let actual = explain_contract_view(&json);
    let expected = serde_json::json!({
        "has_semantic_plan": true,
        "has_optimization_trace": true,
        "has_optimized_plan": true,
        "has_execution_plan": true,
        "optimization_trace": [],
        "semantic_summary": {
            "total_requests": 1,
            "total_responses": 1,
            "total_errors": 0,
            "error_expected": false,
            "assertion_blocks": 0,
            "variable_extractions": 0,
            "rpc_mode_name": "Unary"
        },
        "optimized_summary": {
            "total_requests": 1,
            "total_responses": 1,
            "total_errors": 0,
            "error_expected": false,
            "assertion_blocks": 0,
            "variable_extractions": 0,
            "rpc_mode_name": "Unary"
        },
        "execution_summary": {
            "total_requests": 1,
            "total_responses": 1,
            "total_errors": 0,
            "error_expected": false,
            "assertion_blocks": 0,
            "variable_extractions": 0,
            "rpc_mode_name": "Unary"
        },
        "semantic_target": {
            "endpoint": "example.v1.Greeter/SayHello",
            "package": "example.v1",
            "service": "Greeter",
            "method": "SayHello"
        },
        "optimized_target": {
            "endpoint": "example.v1.Greeter/SayHello",
            "package": "example.v1",
            "service": "Greeter",
            "method": "SayHello"
        },
        "execution_target": {
            "endpoint": "example.v1.Greeter/SayHello",
            "package": "example.v1",
            "service": "Greeter",
            "method": "SayHello"
        }
    });

    assert_eq!(actual, expected);
}

#[test]
fn test_inssect_sections_have_required_fields() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["inspect", &file, "--format", "json"]);
    let json = parse_json_stdout(&output);

    let sections = json["ast"]["sections"]
        .as_array()
        .expect("sections should be array");
    for section in sections {
        assert!(section.get("section_type").is_some());
        assert!(section.get("start_line").is_some());
        assert!(section.get("end_line").is_some());
        assert!(section.get("content_kind").is_some());
    }
}

#[test]
fn test_fmt_check_mode_ok_output() {
    let file = fixture_path("tests/data/gctf/valid_simple.gctf");
    let output = run_cli(&["fmt", &file]);
    assert!(
        output.status.success(),
        "fmt command failed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("... OK"));
}

#[test]
fn test_fmt_check_mode_fails_when_format_needed() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("needs-format.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name":"World"}

--- RESPONSE ---
{"message":"Hello World"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", &path]);

    assert!(
        !output.status.success(),
        "fmt check mode must fail when formatting is needed\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FORMAT_NEEDED]"));
}

#[test]
fn test_fmt_write_then_check_is_idempotent() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("idempotent.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");
    let path = file.to_string_lossy().into_owned();

    let write_output = run_cli(&["fmt", "-w", &path]);
    assert!(write_output.status.success());

    let output1 = run_cli(&["fmt", &path]);
    let output2 = run_cli(&["fmt", &path]);

    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);

    assert_eq!(stdout1, stdout2, "Formatter should be idempotent");
}

#[test]
fn test_fmt_fails_when_check_fails() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("invalid-plugin.gctf");
    let content = r#"--- ENDPOINT ---
example.v1.Greeter/SayHello

--- REQUEST ---
{"name": "World"}

--- RESPONSE ---
{"message": "Hello World"}

--- ASSERTS ---
@regexp(.message, /World/) == true
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let path = file.to_string_lossy().into_owned();
    let output = run_cli(&["fmt", &path]);

    assert!(
        !output.status.success(),
        "fmt must fail when check fails\nstderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_list_json_output() {
    let dir = fixture_path("tests/data/gctf");
    let output = run_cli(&["list", &dir, "--format", "json"]);
    let json = parse_json_stdout(&output);

    assert!(json.get("tests").is_some());
    let tests = json["tests"].as_array().expect("tests should be array");
    assert!(!tests.is_empty());

    for test in tests {
        assert!(test.get("id").is_some());
        assert!(test.get("label").is_some());
        assert!(test.get("uri").is_some());
    }
}

#[test]
fn test_list_with_range() {
    let dir = fixture_path("tests/data/gctf");
    let output = run_cli(&["list", &dir, "--format", "json", "--with-range"]);
    let json = parse_json_stdout(&output);

    let tests = json["tests"].as_array().expect("tests should be array");
    for test in tests {
        if test.get("range").is_some() {
            let range = &test["range"];
            assert!(range.get("start").is_some());
            assert!(range.get("end").is_some());
        }
    }
}
