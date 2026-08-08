#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! A RESPONSE or ERROR section with no body must be rejected.
//!
//! Regression: `expected_values_for_response_section` mapped
//! `SectionContent::Empty` to an empty `Vec` through its catch-all arm, so the
//! expectation loop ran zero times and the runner reported a pass having
//! asserted nothing. `check` accepted the same file, because content
//! validation only inspected the `Json`/`JsonLines` arms. That is the shape a
//! deleted snapshot or a botched `--write` leaves behind: a permanently green
//! no-op test.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn write_doc(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let file = dir.join(name);
    std::fs::write(&file, body).unwrap();
    file
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_response_section_fails_the_run() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = write_doc(
        dir.path(),
        "empty-response.gctf",
        &format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n"
        ),
    );

    let output = cli_command()
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        !output.status.success(),
        "an empty RESPONSE asserts nothing and must not pass: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_rejects_an_empty_response_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_doc(
        dir.path(),
        "empty-response.gctf",
        "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n",
    );

    let output = cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");

    assert!(
        !output.status.success(),
        "check must reject an empty RESPONSE: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_rejects_an_empty_error_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_doc(
        dir.path(),
        "empty-error.gctf",
        "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ERROR ---\n",
    );

    let output = cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");

    assert!(
        !output.status.success(),
        "check must reject an empty ERROR: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The documented way to accept any response stays valid.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_with_an_empty_object_still_passes() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = write_doc(
        dir.path(),
        "partial.gctf",
        &format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    );

    let output = cli_command()
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
