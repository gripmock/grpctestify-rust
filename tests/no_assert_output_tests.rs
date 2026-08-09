#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `--no-assert` means "measure the transport". It used to pretty-print and
//! print every response body, which is the opposite.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn document(address: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_assert_prints_no_response_bodies() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(&file, document(&address)).unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--no-assert",
            "--requests",
            "50",
            "--concurrency",
            "4",
        ])
        .output()
        .expect("failed to run bench");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RESPONSE (Raw)"),
        "--no-assert must not print response bodies:\n{stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verbose_still_shows_the_response() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(&file, document(&address)).unwrap();

    let output = cli_command()
        .args(["run", &file.to_string_lossy(), "--verbose"])
        .output()
        .expect("failed to run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("RESPONSE (Raw)"),
        "-v must still show the response body:\n{stdout}"
    );
}
