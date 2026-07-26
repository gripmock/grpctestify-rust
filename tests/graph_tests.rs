#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::run_cli;

fn write(dir: &std::path::Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("failed to write fixture");
}

const UNARY: &str =
    "--- ENDPOINT ---\nsvc.Thing/Do\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";

#[test]
fn graph_text_shows_setup_tests_teardown_in_order() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "_setup.gctf", UNARY);
    write(dir.path(), "a.gctf", UNARY);
    write(dir.path(), "b.gctf", UNARY);
    write(dir.path(), "_teardown.gctf", UNARY);

    let output = run_cli(&["graph", &dir.path().to_string_lossy()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let setup = stdout.find("_setup.gctf (setup)").unwrap();
    let a = stdout.find("a.gctf").unwrap();
    let b = stdout.find("b.gctf").unwrap();
    let teardown = stdout.find("_teardown.gctf (teardown)").unwrap();
    assert!(setup < a && a < b && b < teardown, "stdout: {stdout}");
}

#[test]
fn graph_mermaid_wraps_output_in_a_fenced_block() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.gctf", UNARY);

    let output = run_cli(&[
        "graph",
        &dir.path().to_string_lossy(),
        "--format",
        "mermaid",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("```mermaid"));
    assert!(stdout.contains("flowchart TD"));
    assert!(stdout.contains("a.gctf"));
}

#[test]
fn graph_annotates_multi_document_chains_with_step_methods() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "chain.gctf",
        "--- ENDPOINT ---\nshop.Cart/AddItem\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\nshop.Cart/Checkout\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
    );

    let output = run_cli(&["graph", &dir.path().to_string_lossy()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("chain.gctf (AddItem → Checkout)"),
        "stdout: {stdout}"
    );
}

#[test]
fn graph_on_directory_with_no_test_files_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_cli(&["graph", &dir.path().to_string_lossy()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No .gctf test files found"));
}
