#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! A benchmark with nothing to verify never reads the response: it reports
//! requests sent rather than completed, and leaves one abandoned call per
//! request behind.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn document(address: &str, verification: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 20\nconcurrency: 2\n\n--- REQUEST ---\n{{}}\n{verification}"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_document_with_nothing_to_verify_is_refused() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("blind.gctf");
    std::fs::write(&file, document(&address, "")).unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");

    assert!(!output.status.success(), "bench must refuse the document");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verification section"),
        "the error must name what is missing: {stderr}"
    );
    assert!(
        stderr.contains("invalid test document"),
        "bench must report it as a document validation failure: {stderr}"
    );
    assert!(
        stderr.contains("blind.gctf"),
        "the error must name the file: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_partial_response_section_is_enough() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ok.gctf");
    let report = dir.path().join("report.json");
    std::fs::write(
        &file,
        document(&address, "\n--- RESPONSE partial ---\n{}\n"),
    )
    .unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--log-format",
            "json",
            "--log-output",
            &report.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(json["summary"]["count"].as_u64().unwrap(), 20);
    assert_eq!(json["summary"]["passed"].as_u64().unwrap(), 20);
}
