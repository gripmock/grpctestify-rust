#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! `bench` must not accept a configuration it then quietly ignores.
//!
//! Each case here covers a flag or key that was parsed, reported, and then had
//! no effect — or one whose malformed value was swallowed and replaced with a
//! default.

#[path = "support/mod.rs"]
mod support;
use support::{cli_command, spawn_health_server};

fn doc(address: &str, extra_bench: &str, meta: &str) -> String {
    format!(
        "{meta}--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- BENCH ---\nmode: fixed\nrequests: 3\nconcurrency: 1\n{extra_bench}\n--- REQUEST ---\n{{}}\n\n--- RESPONSE partial ---\n{{}}\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn console_format_honours_log_output() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    let out = dir.path().join("report.txt");
    std::fs::write(&file, doc(&address, "", "")).unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--log-output",
            &out.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");
    assert!(output.status.success());

    assert!(
        out.exists(),
        "the console arm ignored --log-output while every other format honoured it"
    );
    assert!(!std::fs::read_to_string(&out).unwrap().trim().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_custom_template_still_reaches_the_threshold_gate() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    let template = dir.path().join("tpl.txt");
    std::fs::write(&file, doc(&address, "thresholds.rps: > 100000000\n", "")).unwrap();
    std::fs::write(&template, "count={{ summary.count }}").unwrap();

    let output = cli_command()
        .args([
            "bench",
            &file.to_string_lossy(),
            "--report-template",
            &template.to_string_lossy(),
        ])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "--report-template returned before the gates, so a violated threshold exited 0"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_sources_yaml_is_an_error() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(
        &file,
        doc(&address, "sources: [this is not a source]\n", ""),
    )
    .unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "a malformed sources block used to be ignored, leaving placeholders unsubstituted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_out_of_range_sample_rate_is_an_error() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(&file, doc(&address, "sample_rate: abc\n", "")).unwrap();

    let output = cli_command()
        .args(["bench", &file.to_string_lossy()])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "an unparsable sample_rate silently became 1.0 (sample everything)"
    );
}

#[test]
fn a_negative_duration_is_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(&file, doc("127.0.0.1:1", "duration: -30s\n", "")).unwrap();

    let output = cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");

    assert!(
        !output.status.success(),
        "a negative duration reached the runtime and became a zero-length run"
    );
}

#[test]
fn request_timeout_is_a_known_bench_key() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(&file, doc("127.0.0.1:1", "request_timeout: 120s\n", "")).unwrap();

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
        !combined.contains("request_timeout"),
        "request_timeout is honoured at runtime and must not be reported as unknown: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tag_filters_select_which_files_run() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("wanted.gctf"),
        doc(&address, "", "--- META ---\nname: wanted\ntags: [keep]\n\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("unwanted.gctf"),
        doc(
            "127.0.0.1:1",
            "",
            "--- META ---\nname: unwanted\ntags: [drop]\n\n",
        ),
    )
    .unwrap();

    // Only the tagged file points at a live server, so if the filter is
    // ignored the run also fails — assert on the selection, not just the code.
    let output = cli_command()
        .args(["bench", &dir.path().to_string_lossy(), "--tags", "keep"])
        .output()
        .expect("failed to run bench");

    assert!(
        output.status.success(),
        "--tags was accepted and never read, so both files ran: stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = cli_command()
        .args([
            "bench",
            &dir.path().to_string_lossy(),
            "--tags",
            "no-such-tag",
        ])
        .output()
        .expect("failed to run bench");
    assert!(
        !output.status.success(),
        "a tag filter matching nothing must not silently run everything"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conflicting_bench_sections_are_rejected() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.gctf"), doc(&address, "", "")).unwrap();
    std::fs::write(dir.path().join("b.gctf"), doc(&address, "warmup: 1s\n", "")).unwrap();

    let output = cli_command()
        .args(["bench", &dir.path().to_string_lossy()])
        .output()
        .expect("failed to run bench");

    assert!(
        !output.status.success(),
        "the BENCH section used to be taken from the first-sorted file alone"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("conflicting BENCH"),
        "the error must name the conflict: {stderr}"
    );
}
