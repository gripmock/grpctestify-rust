#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! A benchmark of a negative path is a real measurement.
//!
//! Regression: `bench` derived its verdict from the gRPC status code and threw
//! away the document's own result, so a `.gctf` asserting `--- ERROR partial ---`
//! reported `ok=0`, `errors=<count>` and **every latency percentile 0** even
//! though every request behaved exactly as asserted. `bavix/gripmock`'s
//! worst-case ("no stub can match") benchmark hit this: it had to invert the
//! check in jq and could not measure that path's latency at all.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

/// The health service answers `NOT_FOUND` for a service it does not know, so
/// this document asserts an error and passes.
fn expected_error_doc(address: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 20\nconcurrency: 2\n\n--- REQUEST ---\n{{\"service\": \"no-such-service\"}}\n\n--- ERROR partial ---\n{{}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expected_error_benchmark_measures_and_passes() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("miss.gctf");
    let report_path = dir.path().join("report.json");
    std::fs::write(&file, expected_error_doc(&address)).unwrap();

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

    assert!(
        output.status.success(),
        "a benchmark whose every request satisfied its ERROR section must exit 0: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    let summary = &report["summary"];
    let count = summary["count"].as_u64().unwrap();

    assert!(count > 0, "expected requests to have been issued");
    assert_eq!(
        summary["passed"].as_u64().unwrap(),
        count,
        "every request satisfied the ERROR section, so all of them passed"
    );
    assert_eq!(summary["failed"].as_u64().unwrap(), 0);

    // The transport view stays truthful and separate.
    assert_eq!(
        summary["ok"].as_u64().unwrap(),
        0,
        "no request returned an OK status"
    );
    assert_eq!(
        report["grpc_status_distribution"]["NotFound"]
            .as_u64()
            .unwrap(),
        count,
        "the real gRPC status is still reported"
    );

    // The point of the fix: this path is now measurable.
    let percentiles = report["latency_distribution"].as_array().unwrap();
    assert!(
        percentiles
            .iter()
            .all(|p| p["latency_ns"].as_u64().unwrap() > 0),
        "latency percentiles must be populated for a passing negative-path run, got: {percentiles:?}"
    );

    let per_endpoint = report["per_endpoint"].as_array().unwrap();
    assert!(
        per_endpoint[0]["latency_p50"].as_u64().unwrap() > 0,
        "per-endpoint percentiles follow the same rule"
    );
}

/// Regression: `request_passes` divides the budget by the document count, so a
/// budget smaller than that count truncated to zero passes — `bench dir/ -n 10`
/// over 20 files issued **no requests**, wrote a report and exited 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_budget_below_the_document_count_is_an_error() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    for i in 0..4 {
        std::fs::write(
            dir.path().join(format!("t{i}.gctf")),
            format!(
                "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
            ),
        )
        .unwrap();
    }

    let output = cli_command()
        .args([
            "bench",
            &dir.path().to_string_lossy(),
            "--requests",
            "2",
            "--concurrency",
            "1",
        ])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "an unsatisfiable request budget must fail loudly"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no requests"),
        "the error must explain that nothing would be issued: {stderr}"
    );
}

/// A run where nothing satisfies its document is still a hard failure — the
/// verdict-based gate must not turn "the target is down" into a pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_that_satisfies_nothing_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dead.gctf");
    std::fs::write(
        &file,
        "--- ADDRESS ---\n127.0.0.1:1\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n\n--- REQUEST ---\n{}\n\n--- RESPONSE partial ---\n{}\n",
    )
    .unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "a run against a dead target must not pass"
    );
}
