#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! The open model spawns one task per arrival. Those tasks hand their outcome
//! back to the driver instead of locking a shared accumulator, so this asserts
//! that nothing is dropped on the way.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

const REQUESTS: u64 = 200;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_open_model_records_every_arrival() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("open.gctf");
    let report_path = dir.path().join("report.json");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: adaptive\nrequests: {REQUESTS}\nconcurrency: 8\nmax_rps: 2000\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--log-format",
            "json",
            "--log-output",
            &report_path.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("open"),
        "the run must have used the open model: stderr:\n{stderr}"
    );
    assert!(output.status.success(), "bench failed: stderr:\n{stderr}");

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    let summary = &report["summary"];

    assert_eq!(
        summary["count"].as_u64().unwrap(),
        REQUESTS,
        "every arrival must reach the metrics: {summary}"
    );
    assert_eq!(summary["ok"].as_u64().unwrap(), REQUESTS);
    assert_eq!(summary["passed"].as_u64().unwrap(), REQUESTS);
    assert!(summary["average_ns"].as_u64().unwrap() > 0);
    assert!(
        summary["slowest_ns"].as_u64().unwrap() >= summary["fastest_ns"].as_u64().unwrap(),
        "latency samples must have been recorded, not just counted: {summary}"
    );
}
