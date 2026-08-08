#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! `run` may only rewrite a `.gctf` under `--write`.
//!
//! Regression: the snapshot-update call was gated on `captured_response`
//! being present, but that field is also populated by `capture_exchange`,
//! which report formats enable. `--log-format json --log-output r.json` —
//! an ordinary CI invocation — therefore overwrote every RESPONSE section
//! with whatever the server happened to return, destroying the assertions
//! the run was meant to check.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

/// A document whose RESPONSE deliberately disagrees with the server.
fn wrong_response_doc(address: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{{\"status\": \"PLACEHOLDER_WRONG\"}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn report_output_does_not_rewrite_the_test_file() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wrong.gctf");
    let report = dir.path().join("report.json");
    let original = wrong_response_doc(&address);
    std::fs::write(&file, &original).unwrap();

    let output = cli_command()
        .args([
            "run",
            &file.to_string_lossy(),
            "--log-format",
            "json",
            "--log-output",
            &report.to_string_lossy(),
        ])
        .output()
        .expect("failed to run");

    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        after, original,
        "run without --write must leave the file byte-identical"
    );
    assert!(
        !output.status.success(),
        "the deliberately wrong RESPONSE must still fail the run: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_mode_still_rewrites_the_test_file() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wrong.gctf");
    let original = wrong_response_doc(&address);
    std::fs::write(&file, &original).unwrap();

    cli_command()
        .args(["run", &file.to_string_lossy(), "--write"])
        .output()
        .expect("failed to run");

    let after = std::fs::read_to_string(&file).unwrap();
    assert_ne!(after, original, "--write must update the snapshot");
    assert!(
        after.contains("SERVING") && !after.contains("PLACEHOLDER_WRONG"),
        "--write must replace the RESPONSE with the actual server reply, got:\n{after}"
    );
}
