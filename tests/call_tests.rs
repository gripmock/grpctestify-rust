#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::cli_command;
use support::fixture_path as fixture;
use support::run_cli as run;
use support::spawn_health_server;

/// §2.2: `call`'s inline mode (`-e`, no file) can drive TLS from CLI flags. The
/// `--plaintext` flag forces a no-TLS connection from inline mode alone (there
/// is no file to carry a TLS section), reaching the spawned plaintext server —
/// proving the CLI-TLS-config path is wired for inline calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_inline_plaintext_flag_connects_to_the_server() {
    let address = spawn_health_server().await;
    let output = cli_command()
        .env("GRPCTESTIFY_ADDRESS", &address)
        .args([
            "call",
            "-e",
            "grpc.health.v1.Health/Check",
            "-d",
            "{}",
            "--plaintext",
        ])
        .output()
        .expect("failed to run call");
    assert!(
        output.status.success(),
        "inline call --plaintext must reach the plaintext server:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SERVING"),
        "expected a health SERVING response: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// §4.1: golden success output, and NO_COLOR-invariance. `call`'s body is
/// deterministic for a health Check (`{"status": "SERVING"}` pretty JSON) and
/// carries no ANSI, so stdout is byte-identical with and without NO_COLOR.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_success_output_is_exact_and_no_color_invariant() {
    let address = spawn_health_server().await;

    let with_color = cli_command()
        .env("GRPCTESTIFY_ADDRESS", &address)
        .args([
            "call",
            "-e",
            "grpc.health.v1.Health/Check",
            "-d",
            "{}",
            "--plaintext",
        ])
        .output()
        .expect("run call");
    assert!(with_color.status.success());
    assert_eq!(
        String::from_utf8_lossy(&with_color.stdout).trim(),
        "{\n  \"status\": \"SERVING\"\n}",
        "exact pretty-JSON body"
    );

    let no_color = cli_command()
        .env("GRPCTESTIFY_ADDRESS", &address)
        .env("NO_COLOR", "1")
        .args([
            "call",
            "-e",
            "grpc.health.v1.Health/Check",
            "-d",
            "{}",
            "--plaintext",
        ])
        .output()
        .expect("run call NO_COLOR");
    assert!(no_color.status.success());
    assert_eq!(
        with_color.stdout, no_color.stdout,
        "call emits no ANSI — stdout must be byte-identical with/without NO_COLOR"
    );
}

/// §4.1 (TLS path): a CLI TLS flag actually reaches the transport's TLS setup —
/// `--tls-ca <missing>` fails while loading the CA, proving the flag is wired
/// into the connection (not silently ignored). (A full happy-path TLS handshake
/// would need a cert-generating TLS server, deferred.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_tls_ca_flag_reaches_transport_tls_setup() {
    let address = spawn_health_server().await;
    let output = cli_command()
        .env("GRPCTESTIFY_ADDRESS", &address)
        .args([
            "call",
            "-e",
            "grpc.health.v1.Health/Check",
            "-d",
            "{}",
            "--tls-ca",
            "/no/such/ca-cert-4f3a.pem",
        ])
        .output()
        .expect("run call");
    assert!(
        !output.status.success(),
        "a missing --tls-ca file must fail (TLS setup reached), not silently succeed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    // -s must suppress all curl-style verbose lines; hard process errors may remain.
    let has_verbose_line = stderr
        .lines()
        .any(|line| line.starts_with("* ") || line.starts_with("> ") || line.starts_with("< "));
    assert!(
        !has_verbose_line,
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
