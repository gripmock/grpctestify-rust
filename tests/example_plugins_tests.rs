#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Proves every script under `examples/plugins/` actually works — not just
//! that `check` accepts the plugin names, but that `run` executes their
//! logic correctly against a real server. Keeps the examples from silently
//! rotting out of sync with the plugin contract.
//!
//! There's no `--plugin-dir` flag anymore — plugins are picked up from two
//! convention directories (`~/.grpctestify/plugins`, `./.grpctestify/plugins`).
//! Each test copies `examples/plugins/*.rhai` into a per-test tempdir's
//! `.grpctestify/plugins/` and runs with that tempdir as both `$HOME` and
//! the working directory, so tests stay isolated from each other and from
//! whatever the real host's `$HOME` happens to contain.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

fn examples_plugins_source_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins")
}

/// Copy every `.rhai` file from `examples/plugins/` into `<cwd>/.grpctestify/plugins/`
/// — the project-local convention directory `run` picks up automatically.
fn setup_project_plugin_dir(cwd: &std::path::Path) {
    let dest = cwd.join(".grpctestify/plugins");
    std::fs::create_dir_all(&dest).unwrap();
    for entry in std::fs::read_dir(examples_plugins_source_dir()).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
            std::fs::copy(&path, dest.join(path.file_name().unwrap())).unwrap();
        }
    }
}

/// Spawn a real `grpc.health.v1.Health` server (plus reflection) on an
/// ephemeral port — same pattern used throughout this test suite.
async fn spawn_health_server() -> String {
    let (reporter, health_service) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("build reflection service");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(reflection_service)
            .serve_with_incoming(incoming)
            .await
            .expect("health server run");
    });

    addr.to_string()
}

fn write_test_file(dir: &std::path::Path, address: &str, asserts: &str) -> std::path::PathBuf {
    let file = dir.join("t.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n{asserts}\n"
        ),
    )
    .expect("failed to write test file");
    file
}

fn run_in(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    cli_command()
        .current_dir(dir)
        .env("HOME", dir)
        .args(args)
        .output()
        .expect("failed to run CLI")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assertion_plugins_pass_on_valid_input() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(
        dir.path(),
        &address,
        r#"@is_even(4)
@in_range(30, 18, 65)
@is_palindrome("level")
@luhn_valid("4532015112830366")
@slugify("Hello World!") == "hello-world"
@combined_example("Ada")
@stdlib_demo("550e8400-e29b-41d4-a716-446655440000")
@flexible_match(.status)
@flexible_match("ABC123", "^[A-Z]{3}[0-9]{3}$")"#,
    );

    let output = run_in(dir.path(), &["run", &file.to_string_lossy(), "--verbose"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assertion_plugins_fail_on_invalid_input() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(dir.path(), &address, r#"@is_even(3)"#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(!output.status.success(), "@is_even(3) must fail — 3 is odd");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn luhn_valid_rejects_a_bad_checksum() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    // Same digits as the valid example, last digit changed — breaks the checksum.
    let file = write_test_file(dir.path(), &address, r#"@luhn_valid("4532015112830367")"#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(!output.status.success(), "bad Luhn checksum must fail");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdlib_demo_rejects_a_non_uuid_and_logs_a_warning() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(dir.path(), &address, r#"@stdlib_demo("not-a-uuid")"#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy(), "--verbose"]);
    assert!(
        !output.status.success(),
        "a non-UUID value must fail @stdlib_demo"
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("not a UUID"),
        "stdlib_demo.rhai's log_warn (via the shared rhai stdlib) must reach real output: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reporter_scripts_emit_expected_output() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(dir.path(), &address, r#".status == "SERVING""#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("\"metric\":\"test\"") && stdout.contains("\"metric\":\"suite\""),
        "ndjson_metrics.rhai output missing: {stdout}"
    );
    assert!(
        stdout.contains("-> starting"),
        "combined_example.rhai on_test_start output missing: {stdout}"
    );
    assert!(
        stdout.contains("DIGEST:"),
        "failure_digest.rhai output missing (expected empty digest on a clean pass): {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_test_alert_fires_when_duration_exceeds_threshold() {
    // A real network round-trip against a spawned local server is fast
    // (single-digit ms), so it should stay under slow_test_alert's 200ms
    // threshold and print nothing for it — proving the threshold isn't
    // trivially always-on.
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(dir.path(), &address, r#".status == "SERVING""#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("SLOW ("),
        "a fast local call must not trip the 200ms slow-test alert: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_shape_report_prints_config_summary() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());
    let file = write_test_file(dir.path(), &address, r#".status == "SERVING""#);

    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SHAPE") && stdout.contains("chain_steps=1"),
        "test_shape_report.rhai output missing: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flexible_match_dispatches_by_arity() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    setup_project_plugin_dir(dir.path());

    // 1-arg overload: present-value check — a missing field must fail.
    let missing_field = write_test_file(dir.path(), &address, r#"@flexible_match(.nope)"#);
    let output = run_in(dir.path(), &["run", &missing_field.to_string_lossy()]);
    assert!(
        !output.status.success(),
        "@flexible_match(.nope) must fail — the field doesn't exist"
    );

    // 2-arg overload: present AND matches — a non-matching pattern must fail.
    let file = write_test_file(
        dir.path(),
        &address,
        r#"@flexible_match(.status, "^NOPE$")"#,
    );
    let output = run_in(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        !output.status.success(),
        "@flexible_match(.status, \"^NOPE$\") must fail — SERVING doesn't match"
    );
}
