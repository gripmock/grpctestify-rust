#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! A worker prepares its client once and reuses it. The failure that matters is
//! a reused client carrying a request to the wrong destination.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn document(address: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn documents_with_different_addresses_keep_their_own_client() {
    let live = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("report.json");

    // One reachable, one not. A client reused across the two would make the
    // unreachable document succeed.
    std::fs::write(dir.path().join("a_live.gctf"), document(&live)).unwrap();
    std::fs::write(dir.path().join("b_dead.gctf"), document("127.0.0.1:1")).unwrap();

    let output = cli_command()
        .args([
            "bench",
            &dir.path().to_string_lossy(),
            "--requests",
            "40",
            "--concurrency",
            "4",
            "--log-format",
            "json",
            "--log-output",
            &report.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let summary = &json["summary"];
    let count = summary["count"].as_u64().unwrap();
    let ok = summary["ok"].as_u64().unwrap();
    let errors = summary["errors"].as_u64().unwrap();

    assert_eq!(ok + errors, count);
    assert!(
        ok > 0,
        "the reachable document must succeed: {summary}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        errors > 0,
        "the unreachable document must still fail — a prepared client must not \
         be reused across addresses: {summary}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_requests_against_one_address_all_succeed() {
    let live = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("one.gctf");
    let report = dir.path().join("report.json");
    std::fs::write(&file, document(&live)).unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--requests",
            "200",
            "--concurrency",
            "8",
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
    assert_eq!(json["summary"]["ok"].as_u64().unwrap(), 200);
    assert_eq!(json["summary"]["passed"].as_u64().unwrap(), 200);
}
