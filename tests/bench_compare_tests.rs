#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `bench-compare` had no test coverage at all, despite being the command that
//! gates performance regressions in CI: if it stopped failing on a real
//! regression, nothing would notice — a green pipeline is exactly what a broken
//! gate looks like.
//!
//! These drive it through its documented contract only: two
//! `bench_report_schema_v1` JSON reports in, an exit code out.

#[path = "support/mod.rs"]
mod support;

/// Minimal report in the shape `bench-compare` reads: `summary.{count, errors,
/// rps_observed, average_ns}`, plus the percentile and per-endpoint arrays.
fn report(rps: f64, mean_ns: f64, p99_ns: f64, errors: u64, count: u64) -> String {
    serde_json::json!({
        "summary": {
            "count": count,
            "errors": errors,
            "rps_observed": rps,
            "average_ns": mean_ns,
        },
        "latency_distribution": [
            { "percentile": "p50", "latency_ns": mean_ns },
            { "percentile": "p99", "latency_ns": p99_ns },
        ],
        "per_endpoint": [
            { "endpoint": "svc.Service/Method", "latency_p99": p99_ns },
        ],
    })
    .to_string()
}

fn write_pair(baseline: &str, current: &str) -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let b = dir.path().join("baseline.json");
    let c = dir.path().join("current.json");
    std::fs::write(&b, baseline).unwrap();
    std::fs::write(&c, current).unwrap();
    let (bs, cs) = (
        b.to_string_lossy().into_owned(),
        c.to_string_lossy().into_owned(),
    );
    (dir, bs, cs)
}

#[test]
fn identical_reports_pass_the_gate() {
    let r = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    let (_dir, baseline, current) = write_pair(&r, &r);

    let output = support::cli_command()
        .args(["bench-compare", &baseline, &current])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        output.status.success(),
        "a report compared against itself must pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The point of the command: a latency rise past the threshold must fail.
#[test]
fn latency_regression_fails_the_gate() {
    let baseline = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    // Both mean and p99 doubled — far past the default 10% tolerance.
    let current = report(1000.0, 2_000_000.0, 4_000_000.0, 0, 100);
    let (_dir, b, c) = write_pair(&baseline, &current);

    let output = support::cli_command()
        .args(["bench-compare", &b, &c])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        !output.status.success(),
        "doubled latency must fail the gate\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// ...and the threshold must be a threshold, not decoration: the same
/// regression passes when the tolerance is raised above it.
#[test]
fn latency_regression_passes_when_tolerance_allows_it() {
    let baseline = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    let current = report(1000.0, 2_000_000.0, 4_000_000.0, 0, 100);
    let (_dir, b, c) = write_pair(&baseline, &current);

    let output = support::cli_command()
        .args([
            "bench-compare",
            &b,
            &c,
            "--max-latency-regression",
            "150",
            "--min-throughput",
            "100",
        ])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        output.status.success(),
        "100% slower is within a 150% tolerance\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn throughput_drop_fails_the_gate() {
    let baseline = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    // Half the requests per second, latency unchanged.
    let current = report(500.0, 1_000_000.0, 2_000_000.0, 0, 100);
    let (_dir, b, c) = write_pair(&baseline, &current);

    let output = support::cli_command()
        .args(["bench-compare", &b, &c])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        !output.status.success(),
        "halved throughput must fail the gate\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn error_rate_rise_fails_the_gate() {
    let baseline = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    // 20 errors in 100 requests against a 1-percentage-point tolerance.
    let current = report(1000.0, 1_000_000.0, 2_000_000.0, 20, 100);
    let (_dir, b, c) = write_pair(&baseline, &current);

    let output = support::cli_command()
        .args(["bench-compare", &b, &c])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        !output.status.success(),
        "a 20-point error-rate rise must fail the gate\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// An improvement is not a regression.
#[test]
fn faster_and_higher_throughput_passes() {
    let baseline = report(1000.0, 2_000_000.0, 4_000_000.0, 0, 100);
    let current = report(1500.0, 1_000_000.0, 2_000_000.0, 0, 100);
    let (_dir, b, c) = write_pair(&baseline, &current);

    let output = support::cli_command()
        .args(["bench-compare", &b, &c])
        .output()
        .expect("failed to run bench-compare");

    assert!(
        output.status.success(),
        "faster with more throughput must pass\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn json_format_emits_parseable_output() {
    let r = report(1000.0, 1_000_000.0, 2_000_000.0, 0, 100);
    let (_dir, b, c) = write_pair(&r, &r);

    let output = support::cli_command()
        .args(["bench-compare", &b, &c, "--format", "json"])
        .output()
        .expect("failed to run bench-compare");

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .unwrap_or_else(|e| panic!("--format json must emit valid JSON: {e}\ngot:\n{stdout}"));
}
