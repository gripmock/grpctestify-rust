#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! An ERROR section with no body must be rejected: an error either happened or
//! it did not, so there is nothing for an empty body to mean.
//!
//! An empty RESPONSE is different — messages there are newline-delimited, so no
//! lines is the natural spelling of no messages. See `empty_response_tests`.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn write_doc(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let file = dir.join(name);
    std::fs::write(&file, body).unwrap();
    file
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
