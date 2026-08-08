#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! `run` must not silently ignore a test path it cannot resolve.
//!
//! Regression: paths were walked with `if is_dir { .. } else if is_file { .. }`
//! and no `else`, so `run suite/ regresion/` (typo) ran only the paths that
//! happened to exist and exited 0 — a mistyped suite in a CI invocation
//! disappeared without a word and the build stayed green.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn passing_doc(address: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{{\"status\": \"SERVING\"}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_nonexistent_path_alongside_a_valid_one_fails_the_run() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.gctf");
    std::fs::write(&good, passing_doc(&address)).unwrap();
    let typo = dir.path().join("regresion.gctf");

    let output = cli_command()
        .args(["run", &good.to_string_lossy(), &typo.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        !output.status.success(),
        "an unresolvable path must fail the run instead of being skipped"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("regresion.gctf"),
        "the error must name the offending path, got:\n{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_valid_path_on_its_own_still_passes() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.gctf");
    std::fs::write(&good, passing_doc(&address)).unwrap();

    let output = cli_command()
        .args(["run", &good.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
