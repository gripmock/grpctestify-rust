#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Golden/snapshot coverage for `inspect`, `explain`, and `graph` output —
//! text and JSON/Mermaid forms. Same convention `golden_output_tests.rs`
//! uses for `run`: non-deterministic content scrubbed to stable
//! placeholders, compared against `tests/golden/`.
//! Regenerate: `UPDATE_GOLDEN=1 cargo test --test golden_inspect_explain_graph_tests`

#[path = "support/mod.rs"]
mod support;

use std::path::Path;

const CHAIN_GCTF: &str = "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE with_asserts ---\n{\"status\": \"SERVING\"}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Watch\n\n--- REQUEST ---\n{}\n\n--- ERROR ---\n{\"code\": 5}\n";

fn scrub(text: &str, tmp_root: &Path) -> String {
    let mut s = text.to_string();

    let tmp_root_str = tmp_root.to_string_lossy().into_owned();
    s = s.replace(&tmp_root_str, "<TMPDIR>");

    let ms = regex::Regex::new(r"\b\d+(\.\d+)?\s?ms\b").unwrap();
    s = ms.replace_all(&s, "<MS>ms").into_owned();

    // JSON float-ms fields: "parse_time_ms": 0.858708
    let json_ms_field =
        regex::Regex::new(r#"("(?:parse_time_ms|validation_time_ms)"\s*:\s*)[\d.]+"#).unwrap();
    s = json_ms_field.replace_all(&s, "${1}0").into_owned();

    s
}

fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/inspect_explain_graph")
        .join(format!("{name}.golden"))
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden file {}: {e}\nrun with UPDATE_GOLDEN=1 to create it\nactual output was:\n{actual}",
            path.display()
        )
    });
    assert_eq!(
        expected, actual,
        "golden mismatch for '{name}' (rerun with UPDATE_GOLDEN=1 if this change is intentional)"
    );
}

fn write_chain(dir: &Path) -> std::path::PathBuf {
    let file = dir.join("chain.gctf");
    std::fs::write(&file, CHAIN_GCTF).expect("write chain.gctf");
    file
}

#[test]
fn golden_inspect_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["inspect", file.to_str().unwrap()])
        .output()
        .expect("failed to run inspect");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("inspect_text", &scrub(&combined, dir.path()));
}

#[test]
fn golden_inspect_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["inspect", file.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run inspect");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("inspect_json", &scrub(&combined, dir.path()));
}

#[test]
fn golden_explain_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["explain", file.to_str().unwrap()])
        .output()
        .expect("failed to run explain");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("explain_text", &scrub(&combined, dir.path()));
}

#[test]
fn golden_explain_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["explain", file.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run explain");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("explain_json", &scrub(&combined, dir.path()));
}

#[test]
fn golden_graph_text() {
    let dir = tempfile::tempdir().unwrap();
    write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["graph", dir.path().to_str().unwrap()])
        .output()
        .expect("failed to run graph");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("graph_text", &scrub(&combined, dir.path()));
}

#[test]
fn golden_graph_mermaid() {
    let dir = tempfile::tempdir().unwrap();
    write_chain(dir.path());
    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(["graph", dir.path().to_str().unwrap(), "--format", "mermaid"])
        .output()
        .expect("failed to run graph");
    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden("graph_mermaid", &scrub(&combined, dir.path()));
}
