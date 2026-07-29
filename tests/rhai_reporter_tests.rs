#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Reporter scripts are picked up from the same convention directories as
//! assertion plugins (no `--plugin-dir` flag) — each test here writes its
//! `.rhai` script into `<dir>/.grpctestify/plugins/` and runs with `dir` as
//! both the cwd and `$HOME`, isolating it from every other test and from
//! the real host's `$HOME`.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

fn write_test_file(dir: &std::path::Path) -> std::path::PathBuf {
    let file = dir.join("t.gctf");
    std::fs::write(
        &file,
        "--- ADDRESS ---\nlocalhost:1\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
    )
    .expect("failed to write test file");
    file
}

/// Write a `.rhai` script into `<dir>/.grpctestify/plugins/` — the
/// project-local convention directory.
fn write_project_plugin(dir: &std::path::Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir.join(".grpctestify/plugins")).unwrap();
    std::fs::write(dir.join(".grpctestify/plugins").join(name), body).unwrap();
}

/// Run the CLI with `dir` as both the working directory and `$HOME`.
fn run_isolated(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    cli_command()
        .current_dir(dir)
        .env("HOME", dir)
        // Script plugins execute only once trusted (`apif_plugins::trust`);
        // a non-interactive test takes the same opt-in CI uses.
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(args)
        .output()
        .expect("failed to run CLI")
}

/// Spawn a real `grpc.health.v1.Health` server (plus reflection) on an
/// ephemeral port — same pattern as `tests/example_plugins_tests.rs`. Used
/// where a test needs a clean Pass (not just "any Fail"), since a down
/// server (`localhost:1`) can fail via more than one code path (connection
/// refused vs. reflection timeout) depending on timing.
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

#[test]
fn on_suite_end_output_reaches_real_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_test_file(dir.path());
    write_project_plugin(
        dir.path(),
        "metrics.rhai",
        r#"fn on_suite_end(results) {
            print(`METRIC suite total=${results.total} failed=${results.failed}`);
        }"#,
    );

    let output = run_isolated(
        dir.path(),
        &["run", &file.to_string_lossy(), "--timeout", "1"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("METRIC suite total=1 failed=1"),
        "reporter script output must reach real stdout: {stdout}"
    );
}

#[test]
fn on_test_end_output_reaches_real_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_test_file(dir.path());
    write_project_plugin(
        dir.path(),
        "per_test.rhai",
        r#"fn on_test_end(name, result) {
            print(`METRIC test status=${result.status}`);
        }"#,
    );

    let output = run_isolated(
        dir.path(),
        &[
            "run",
            &file.to_string_lossy(),
            "--timeout",
            "1",
            "--verbose",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("METRIC test status=Fail"),
        "stdout: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_summary_reaches_the_reporter_script() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .unwrap();
    write_project_plugin(
        dir.path(),
        "summary.rhai",
        r#"fn on_test_end(name, result) {
            let cs = result.config_summary;
            print(`CONFIG sections=${cs.sections.len()} chain_steps=${cs.chain_steps} tls=${cs.tls}`);
        }"#,
    );

    let output = run_isolated(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `tls` is omitted from the serialized map when false (same
    // skip-if-default convention as `retried`), so it reads back as `()` —
    // an empty interpolation, not the literal string "false".
    assert!(
        stdout.contains("sections=4")
            && stdout.contains("chain_steps=1")
            && stdout.contains("tls="),
        "result.config_summary must reach the reporter script: {stdout}"
    );
}

#[test]
fn a_throwing_reporter_does_not_abort_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_test_file(dir.path());
    write_project_plugin(
        dir.path(),
        "broken.rhai",
        r#"fn on_test_end(name, result) { throw "boom"; }"#,
    );

    let output = run_isolated(
        dir.path(),
        &[
            "run",
            &file.to_string_lossy(),
            "--timeout",
            "1",
            "--verbose",
        ],
    );
    // The run itself still completes (and reports the real test failure via
    // its own exit code) — a broken reporter script must not crash the CLI.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("boom") || combined.contains("on_test_end error"),
        "reporter error must be logged, not silently dropped: {combined}"
    );
}

#[test]
fn assertion_only_script_is_not_also_loaded_as_a_reporter() {
    // A script with no Reporter hooks (an assertion plugin) must not cause
    // any reporter-side error/log noise even under --verbose.
    let dir = tempfile::tempdir().unwrap();
    let file = write_test_file(dir.path());
    write_project_plugin(
        dir.path(),
        "is_present.rhai",
        "fn is_present(value) { value != () }",
    );

    let output = run_isolated(
        dir.path(),
        &[
            "run",
            &file.to_string_lossy(),
            "--timeout",
            "1",
            "--verbose",
        ],
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("on_suite_end error") && !combined.contains("on_test_end error"),
        "assertion-only script must not be treated as a broken reporter: {combined}"
    );
}
