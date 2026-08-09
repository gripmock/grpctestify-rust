#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! A concurrency sweep is one run, one report.
//!
//! `bench` had a load (RPS) schedule but no concurrency schedule, so measuring
//! the same scenario at several worker counts meant driving the binary from a
//! shell loop and stitching the JSON back together afterwards — which is
//! exactly what `bavix/gripmock`'s `bench/run.sh` does with bash and `jq`.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn doc(address: &str, bench: &str) -> String {
    format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\n{bench}\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
    )
}

fn run_report(file: &std::path::Path, out: &std::path::Path, extra: &[&str]) -> serde_json::Value {
    let mut args: Vec<String> = vec![
        "bench".into(),
        file.to_string_lossy().into_owned(),
        "--log-format".into(),
        "json".into(),
        "--log-output".into(),
        out.to_string_lossy().into_owned(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_string()));

    let output = cli_command()
        .args(&args)
        .output()
        .expect("failed to run bench");
    assert!(
        output.status.success(),
        "bench failed: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_sweep_reports_every_level() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sweep.gctf");
    let out = dir.path().join("report.json");
    std::fs::write(
        &file,
        doc(
            &address,
            "mode: fixed\nrequests: 6\nconcurrency_schedule: step\nconcurrency_start: 1\nconcurrency_end: 5\nconcurrency_step: 2\n",
        ),
    )
    .unwrap();

    let report = run_report(&file, &out, &[]);
    let levels = report["levels"].as_array().expect("levels array");

    let measured: Vec<u64> = levels
        .iter()
        .map(|l| l["concurrency"].as_u64().unwrap())
        .collect();
    assert_eq!(
        measured,
        vec![1, 3, 5],
        "each configured level must be measured, in order"
    );

    for level in levels {
        assert!(
            level["summary"]["count"].as_u64().unwrap() > 0,
            "every level carries its own summary"
        );
        assert!(
            !level["latency_distribution"].as_array().unwrap().is_empty(),
            "every level carries its own percentiles"
        );
    }

    // The top-level summary aggregates the sweep rather than reporting one level.
    let total: u64 = levels
        .iter()
        .map(|l| l["summary"]["count"].as_u64().unwrap())
        .sum();
    assert_eq!(report["summary"]["count"].as_u64().unwrap(), total);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_const_schedule_reports_no_levels() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("single.gctf");
    let out = dir.path().join("report.json");
    std::fs::write(
        &file,
        doc(&address, "mode: fixed\nrequests: 3\nconcurrency: 2\n"),
    )
    .unwrap();

    let report = run_report(&file, &out, &[]);
    assert!(
        report.get("levels").is_none(),
        "a single-level run must report exactly as it did before the sweep existed"
    );
    assert_eq!(report["summary"]["count"].as_u64().unwrap(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sweep_is_configurable_from_the_cli() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cli.gctf");
    let out = dir.path().join("report.json");
    std::fs::write(&file, doc(&address, "mode: fixed\nrequests: 4\n")).unwrap();

    let report = run_report(
        &file,
        &out,
        &[
            "--concurrency-schedule",
            "line",
            "--concurrency-start",
            "1",
            "--concurrency-end",
            "3",
            "--concurrency-step",
            "1",
        ],
    );

    let measured: Vec<u64> = report["levels"]
        .as_array()
        .expect("levels array")
        .iter()
        .map(|l| l["concurrency"].as_u64().unwrap())
        .collect();
    assert_eq!(measured, vec![1, 2, 3]);
}

#[test]
fn check_rejects_an_unknown_concurrency_schedule() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bad.gctf");
    std::fs::write(
        &file,
        doc(
            "127.0.0.1:1",
            "mode: fixed\nrequests: 1\nconcurrency_schedule: sine\n",
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");

    assert!(
        !output.status.success(),
        "the concurrency schedule needs the same value-set validation as load_schedule"
    );
}

/// The point of the sweep plus aggregation: a charting step reads one file
/// instead of finding, ordering and stitching a directory of reports.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_fold_into_one_matrix() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sweep.gctf");
    std::fs::write(
        &file,
        doc(
            &address,
            "mode: fixed\nrequests: 4\nconcurrency_schedule: step\nconcurrency_start: 1\nconcurrency_end: 3\nconcurrency_step: 2\n",
        ),
    )
    .unwrap();

    // Two runs, as a real comparison would produce (two engines, two datasets…).
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    run_report(&file, &a, &[]);
    run_report(&file, &b, &[]);

    let csv_out = dir.path().join("matrix.csv");
    let output = cli_command()
        .args([
            "bench-aggregate",
            &a.to_string_lossy(),
            &b.to_string_lossy(),
            "--format",
            "csv",
            "-o",
            &csv_out.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench-aggregate");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let csv = std::fs::read_to_string(&csv_out).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert!(
        lines[0].starts_with("run,concurrency,count,"),
        "got: {}",
        lines[0]
    );
    // Two runs × two levels.
    assert_eq!(
        lines.len(),
        5,
        "expected a header plus one row per run per level, got:\n{csv}"
    );

    let json_output = cli_command()
        .args([
            "bench-aggregate",
            &a.to_string_lossy(),
            &b.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench-aggregate");
    let parsed: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("valid matrix JSON");
    assert_eq!(parsed["points"].as_array().unwrap().len(), 4);
}
