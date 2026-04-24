#![cfg(not(miri))]

use std::path::Path;
use std::process::{Command, Output};

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

fn fixture(rel: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

fn run(args: &[&str]) -> Output {
    let binary = get_binary();
    let runner = std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER")
        .ok()
        .or_else(|| std::env::var("CROSS_RUNNER").ok());

    let mut cmd = if let Some(runner) = runner {
        let mut parts = runner.split_whitespace();
        let prog = parts.next().expect("runner must not be empty");
        let mut c = Command::new(prog);
        c.args(parts).arg(&binary);
        c
    } else {
        Command::new(&binary)
    };

    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("failed to run grpctestify")
}

// ── error cases ────────────────────────────────────────────────────────────────

#[test]
fn call_missing_file_returns_error() {
    let out = run(&["call", "nonexistent.gctf"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found") || stderr.contains("No such"));
}

#[test]
fn call_doc_index_zero_is_rejected() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", "--doc-index", "0", &f]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--doc-index must be >= 1"));
}

#[test]
fn call_doc_index_out_of_range_returns_error() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", "--doc-index", "99", &f]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("out of range"));
}

// ── verbose output format ──────────────────────────────────────────────────────

#[test]
fn call_verbose_prints_request_section_to_stderr() {
    let f = fixture("tests/data/call/unreachable.gctf");
    // The call will fail (no server), but verbose output is printed before the connection attempt.
    let out = run(&["call", "-v", &f]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // curl-style: * prefix for info lines
    assert!(
        stderr.contains("* Trying"),
        "expected '* Trying' in stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("EchoService/SayHello"),
        "expected method in stderr:\n{}",
        stderr
    );
}

#[test]
fn call_very_verbose_implies_verbose() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", "--vv", &f]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("* Trying"),
        "expected '* Trying' in stderr:\n{}",
        stderr
    );
}

#[test]
fn call_no_verbose_no_request_section_in_stderr() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", &f]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Without -v the curl-style info lines must not appear.
    assert!(
        !stderr.contains("* Trying") && !stderr.contains("* Connected"),
        "unexpected verbose output in non-verbose mode:\n{}",
        stderr
    );
}

// ── silent mode ────────────────────────────────────────────────────────────────

#[test]
fn call_silent_suppresses_verbose_stderr() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", "-v", "-s", &f]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // -s must suppress all verbose lines; only the hard process error may remain
    assert!(
        !stderr.contains("* Trying") && !stderr.contains("> ") && !stderr.contains("< "),
        "verbose output leaked through silent mode:\n{}",
        stderr
    );
}

#[test]
fn call_silent_produces_no_body_in_stdout() {
    let f = fixture("tests/data/call/unreachable.gctf");
    let out = run(&["call", "-s", &f]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // tracing WARN lines may appear; actual response body (JSON) must not
    let has_json_body = stdout
        .lines()
        .any(|l| l.trim_start().starts_with('{') || l.trim_start().starts_with('['));
    assert!(
        !has_json_body,
        "response body must not appear in silent mode:\n{}",
        stdout
    );
}

// ── multi-doc ──────────────────────────────────────────────────────────────────

#[test]
fn call_multi_doc_selects_by_index() {
    let f = fixture("tests/data/call/multi_doc.gctf");
    // doc-index 1 and 2 should both fail with a connection error, not an "out of range" error
    for idx in ["1", "2"] {
        let out = run(&["call", "--doc-index", idx, &f]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("out of range"),
            "doc-index {} reported out-of-range unexpectedly:\n{}",
            idx,
            stderr
        );
    }
}

#[test]
fn call_multi_doc_index_3_is_out_of_range() {
    let f = fixture("tests/data/call/multi_doc.gctf");
    let out = run(&["call", "--doc-index", "3", &f]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("out of range"), "stderr:\n{}", stderr);
}
