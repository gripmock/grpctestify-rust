#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::run_cli;

fn write(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("failed to write fixture");
    path
}

const DATASET_GCTF: &str = r#"--- ADDRESS ---
localhost:1

--- ENDPOINT ---
users.UserService/GetUser

--- DATASET ---
- id: "1"
  name: Ada
- id: "2"
  name: Grace

--- REQUEST ---
{ "id": "{{dataset.id}}" }

--- RESPONSE ---
{ "id": "{{dataset.id}}", "name": "{{dataset.name}}" }
"#;

#[test]
fn run_expands_dataset_into_one_test_per_row() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "users.gctf", DATASET_GCTF);

    let output = run_cli(&["run", &file.to_string_lossy(), "--dry-run"]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Running 2 tests"), "stdout: {stdout}");
}

#[test]
fn run_substitutes_dataset_fields_per_row() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "users.gctf", DATASET_GCTF);

    // No server is listening — the run fails, but the failure identity
    // proves each row's `dataset.*` vars were actually substituted in.
    let output = run_cli(&[
        "run",
        &file.to_string_lossy(),
        "--timeout",
        "1",
        "--verbose",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dataset.id=1"), "stdout: {stdout}");
    assert!(stdout.contains("dataset.name=Ada"), "stdout: {stdout}");
    assert!(stdout.contains("dataset.id=2"), "stdout: {stdout}");
    assert!(stdout.contains("dataset.name=Grace"), "stdout: {stdout}");
}

#[test]
fn run_rejects_data_flag_combined_with_dataset_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "users.gctf", DATASET_GCTF);
    let data_file = write(dir.path(), "extra.csv", "col\nval\n");

    let output = run_cli(&[
        "run",
        &file.to_string_lossy(),
        "--data",
        &data_file.to_string_lossy(),
    ]);
    assert!(
        !output.status.success(),
        "combining --data with a DATASET section must be a hard error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DATASET") && stderr.contains("--data"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_rejects_write_with_dataset_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "users.gctf", DATASET_GCTF);

    let output = run_cli(&["run", &file.to_string_lossy(), "--write"]);
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("--write"), "output: {combined}");
}

#[test]
fn check_flags_dataset_before_address_as_section_order() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(
        dir.path(),
        "scrambled.gctf",
        r#"--- ADDRESS ---
localhost:1

--- DATASET ---
- id: "1"

--- ENDPOINT ---
users.UserService/GetUser

--- REQUEST ---
{ "id": "{{dataset.id}}" }

--- RESPONSE ---
{}
"#,
    );

    let output = run_cli(&["check", &file.to_string_lossy(), "--format", "json"]);
    let json = support::parse_json_stdout_any_status(&output);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    let order_diag = diagnostics
        .iter()
        .find(|d| d["code"].as_str() == Some("SECTION_ORDER"))
        .expect("SECTION_ORDER diagnostic");
    let message = order_diag["message"].as_str().unwrap();
    assert!(
        message.contains("DATASET should come before ADDRESS"),
        "message: {message}"
    );
}

#[test]
fn fmt_moves_dataset_into_canonical_preamble_position() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(
        dir.path(),
        "scrambled.gctf",
        r#"--- ADDRESS ---
localhost:1

--- DATASET ---
# only ada
- id: "1"

--- ENDPOINT ---
users.UserService/GetUser

--- REQUEST ---
{ "id": "{{dataset.id}}" }

--- RESPONSE ---
{}
"#,
    );

    let output = run_cli(&["fmt", "-w", &file.to_string_lossy()]);
    assert!(output.status.success(), "{:?}", output);

    let formatted = std::fs::read_to_string(&file).unwrap();
    let dataset_pos = formatted.find("--- DATASET ---").unwrap();
    let address_pos = formatted.find("--- ADDRESS ---").unwrap();
    assert!(
        dataset_pos < address_pos,
        "DATASET must move before ADDRESS: {formatted}"
    );
    assert!(
        formatted.contains("# only ada"),
        "YAML comment must survive fmt: {formatted}"
    );
}

#[test]
fn check_warns_on_a_large_dataset() {
    let dir = tempfile::tempdir().unwrap();
    let rows: String = (0..60)
        .map(|i| format!("- id: \"{i}\"\n"))
        .collect::<Vec<_>>()
        .join("");
    let content = format!(
        "--- ENDPOINT ---\nusers.UserService/GetUser\n\n--- DATASET ---\n{rows}\n--- REQUEST ---\n{{ \"id\": \"{{{{dataset.id}}}}\" }}\n\n--- RESPONSE ---\n{{}}\n"
    );
    let file = write(dir.path(), "big.gctf", &content);

    let output = run_cli(&["check", &file.to_string_lossy(), "--format", "json"]);
    let json = support::parse_json_stdout_any_status(&output);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"].as_str() == Some("LARGE_DATASET")),
        "diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn check_does_not_warn_on_a_small_dataset() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "small.gctf", DATASET_GCTF);

    let output = run_cli(&["check", &file.to_string_lossy(), "--format", "json"]);
    let json = support::parse_json_stdout_any_status(&output);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        !diagnostics
            .iter()
            .any(|d| d["code"].as_str() == Some("LARGE_DATASET")),
        "diagnostics: {diagnostics:#?}"
    );
}
