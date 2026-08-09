#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Field-level coverage depends on message types the runner resolves from the
//! descriptor pool. Those lookups are skipped when no collector is attached, so
//! this asserts they still happen — and still produce fields — when one is.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn coverage_reports_request_fields() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("health.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{\"service\": \"\"}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args([
            "run",
            &file.to_string_lossy(),
            "--coverage",
            "--coverage-format",
            "json",
        ])
        .output()
        .expect("failed to run");
    assert!(
        output.status.success(),
        "run failed: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("coverage JSON expected on stdout");
    let coverage: serde_json::Value =
        serde_json::from_str(stdout[json_start..].trim()).expect("coverage JSON must parse");

    let messages = coverage["messages"]
        .as_array()
        .expect("coverage must list message types");
    let request = messages
        .iter()
        .find(|m| m["message_type"] == "grpc.health.v1.HealthCheckRequest")
        .expect("the request message type must have been resolved from the descriptor pool");
    assert_eq!(
        request["covered_fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["service"],
    );

    // The response type is resolved by the same lookup and must survive too.
    assert!(
        messages
            .iter()
            .any(|m| m["message_type"] == "grpc.health.v1.HealthCheckResponse"),
        "coverage must also resolve the response message type: {coverage}"
    );
    assert!(coverage["field_summary"]["total"].as_u64().unwrap() > 0);
}
