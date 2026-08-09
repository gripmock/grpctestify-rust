#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! A benchmark reports what it cost to produce, so a reader can tell a slow
//! target apart from a saturated generator.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_benchmark_reports_its_own_cpu_cost() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cost.gctf");
    let report_path = dir.path().join("report.json");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 200\nconcurrency: 4\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
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
    assert!(
        output.status.success(),
        "bench failed: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    let cost = &report["client_cost"];
    assert!(
        !cost.is_null(),
        "the report must carry a client_cost block: {report}"
    );

    let cpu = cost["cpu_seconds"].as_f64().unwrap();
    let per_request = cost["cpu_us_per_request"].as_f64().unwrap();
    let cores = cost["cores_used"].as_f64().unwrap();
    let host_cores = cost["host_cores"].as_u64().unwrap();

    assert!(cpu > 0.0, "200 requests cannot cost 0s of CPU");
    assert!(per_request > 0.0 && per_request < 1e6, "{per_request} µs");
    assert!(cores > 0.0, "cores_used must be positive");
    assert!(host_cores >= 1);
    assert!(cost["generator_limited"].is_boolean());

    let count = report["summary"]["count"].as_f64().unwrap();
    let derived = cpu * 1e6 / count;
    assert!(
        (derived - per_request).abs() < 1.0,
        "cpu_us_per_request {per_request} must follow from cpu_seconds {cpu} over {count} requests"
    );
}
