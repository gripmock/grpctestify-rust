#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `bench` correctness: exit code must actually reflect whether the run
//! measured anything real, not just whether the process itself completed.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;
use support::spawn_health_server;

/// Regression: a bench run where every single request fails (broken/
/// unreachable target) used to exit 0 whenever no `BENCH.thresholds` were
/// configured — `thresholds_passed()` is vacuously true on an empty
/// threshold list. Zero successes out of N requests must fail regardless.
#[test]
fn bench_fails_when_every_request_errors_even_without_thresholds() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("all-fail.gctf");
    // Port 1 is a reserved/unassignable port — every connection attempt
    // fails immediately, giving a deterministic 100% error rate.
    std::fs::write(
        &file,
        "--- ADDRESS ---\nlocalhost:1\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
    )
    .unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");
    assert!(
        !output.status.success(),
        "bench must fail when it measured nothing: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("measured nothing"),
        "expected the all-failed explanation in stderr: {stderr}"
    );
}

/// A normal, all-successful bench run (no thresholds configured) must still
/// exit 0 — the new all-errors gate must not fire on a healthy run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_succeeds_when_requests_pass_and_no_thresholds_configured() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("all-pass.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The threshold-gating logic (`report.thresholds_passed()` → `bail!`) had
/// unit-test coverage for parsing and for the passing case, but nothing
/// ever exercised a real violation end-to-end through the CLI's actual exit
/// code — an impossible latency threshold (`p50 < 0`) is always violated on
/// a real run, regardless of how fast it actually is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_fails_when_a_threshold_is_violated() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("threshold-violation.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\nthresholds.latency_ms.p(50): <0\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");
    assert!(
        !output.status.success(),
        "bench must fail when a configured threshold is violated: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("threshold"),
        "expected a threshold-failure explanation in stderr: {stderr}"
    );
}

/// `bench-ergonomics` promised throughput gating, but the threshold evaluator
/// only ever saw the counters — never the run's wall time — so `thresholds.rps`
/// resolved to "unknown metric". These two runs differ only in the direction of
/// an unreachable throughput bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_gates_on_throughput() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();

    let run = |name: &str, threshold: &str| {
        let file = dir.path().join(name);
        std::fs::write(
            &file,
            format!(
                "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 5\nconcurrency: 1\nthresholds.rps: {threshold}\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
            ),
        )
        .unwrap();
        cli_command()
            .args(["bench", &file.to_string_lossy()])
            .output()
            .expect("failed to run bench")
    };

    let unreachable = run("rps-too-high.gctf", "> 100000000");
    assert!(
        !unreachable.status.success(),
        "an unreachable rps floor must fail the run: stderr:\n{}",
        String::from_utf8_lossy(&unreachable.stderr)
    );

    let trivial = run("rps-trivial.gctf", "> 0");
    assert!(
        trivial.status.success(),
        "a trivially satisfied rps floor must pass: stderr:\n{}",
        String::from_utf8_lossy(&trivial.stderr)
    );
}

/// §5.1 report-format coverage. Golden *files* are a poor fit for `bench`
/// (latencies/rps/histogram are non-deterministic — a golden would be almost
/// entirely scrubbed and brittle), so each `--log-format` is verified by its
/// deterministic output *shape* against a real spawned server instead. Runs
/// all formats through one server to keep it cheap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_emits_each_report_format_in_the_expected_shape() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("formats.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();

    let run = |format: &str| {
        let out = cli_command()
            .args(["bench", &file.to_string_lossy(), "--log-format", format])
            .output()
            .expect("failed to run bench");
        assert!(
            out.status.success(),
            "bench --log-format {format} failed: stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // console: the ghz-style summary heading.
    assert!(run("console").contains("Benchmark Summary"));

    // json: parses, and reports the 3 requested successful calls.
    let json: serde_json::Value =
        serde_json::from_str(run("json").trim()).expect("json format must emit valid JSON");
    assert_eq!(json["summary"]["count"], 3);
    assert_eq!(json["summary"]["ok"], 3);

    // csv: the stable header row.
    assert!(run("csv").contains("count,ok,errors,total_ns,average_ns,fastest_ns,slowest_ns,rps"));

    // prometheus: exposition-format metric lines.
    let prom = run("prometheus");
    assert!(prom.contains("grpctestify_bench_count 3"));
    assert!(prom.contains("# TYPE grpctestify_bench_count gauge"));

    // ndjson: one JSON object per response (3 requests → 3 lines).
    let ndjson = run("ndjson");
    let lines: Vec<&str> = ndjson.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 3, "ndjson must emit one line per response");
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line)
            .expect("each ndjson line must be valid JSON");
    }
}

/// §2.3: `bench --allure-output-dir` must emit the shared allure-results
/// contract (per-endpoint `*-result.json` + suite sidecars), not just the raw
/// `benchmark-report.json` — so a benchmark shows up in the same Allure
/// dashboard as `run`'s tests.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_allure_output_dir_emits_the_results_contract() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("allure.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
        ),
    )
    .unwrap();
    let allure_dir = dir.path().join("allure-results");

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--allure-output-dir",
            &allure_dir.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Raw report still written alongside the contract.
    assert!(allure_dir.join("benchmark-report.json").is_file());

    // At least one allure `*-result.json`, and it must be a valid, passing
    // result for the benchmarked endpoint.
    let result_files: Vec<_> = std::fs::read_dir(&allure_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("-result.json"))
        })
        .collect();
    assert!(
        !result_files.is_empty(),
        "expected at least one allure -result.json in {}",
        allure_dir.display()
    );
    let body = std::fs::read_to_string(&result_files[0]).unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid allure result JSON");
    assert_eq!(json["status"], "passed");
    // The Allure reporter renders the method name off the endpoint.
    assert!(
        json["name"].as_str().unwrap_or("").contains("Check"),
        "result name should reflect the benchmarked endpoint: {}",
        json["name"]
    );
}

/// Regression: `call --bench -e ...` (direct-call bench mode, no `.gctf`
/// file needed) built its synthetic file with a literal
/// `--- ADDRESS ---\n<env:GRPCTESTIFY_ADDRESS>` — a string nothing actually
/// resolves (`<env:GRPCTESTIFY_ADDRESS>` is a *display-only* label used
/// elsewhere for `inspect`/`explain` output, not a real placeholder syntax)
/// — so every direct-call bench failed immediately with an invalid-URI
/// error, 100% of the time, regardless of whether `$GRPCTESTIFY_ADDRESS`
/// was actually set correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_bench_direct_mode_reaches_the_real_env_address() {
    let address = spawn_health_server().await;

    let output = cli_command()
        .env("GRPCTESTIFY_ADDRESS", &address)
        .args([
            "call",
            "-e",
            "grpc.health.v1.Health/Check",
            "-d",
            "{}",
            "--bench",
            "--requests",
            "2",
            "--concurrency",
            "1",
        ])
        .output()
        .expect("failed to run call --bench");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
