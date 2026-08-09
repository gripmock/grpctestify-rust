#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! A RESPONSE section with no messages asserts that the stream produced none.
//! It used to skip the check and pass whatever arrived.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_response_fails_when_a_message_arrives() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("expects-nothing.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run");

    assert!(
        !output.status.success(),
        "Health/Check answers, so a RESPONSE expecting no messages must fail"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("expects no messages"),
        "the failure must say what was violated:\n{combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_response_is_still_a_valid_document() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("expects-nothing.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("section is empty"),
        "an empty RESPONSE is zero messages, not a malformed document:\n{combined}"
    );
}
