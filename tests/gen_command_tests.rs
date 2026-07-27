#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

use tonic_health::ServingStatus;

#[path = "support/mod.rs"]
mod support;
use support::run_cli;

/// Spawn a real `grpc.health.v1.Health` server plus reflection-v1 on an
/// ephemeral port, so `gen grpcurl --execute` has a live native target
/// instead of shelling out to a `grpcurl` binary (see memory
/// [[native-zero-dependency]]).
async fn spawn_health_server() -> String {
    let (reporter, health_service) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", ServingStatus::Serving)
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
fn gen_grpcurl_defaults_to_stdout() {
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-plaintext",
        "-H",
        "x-api-key: wrong-key",
        "-d",
        "{\"action\":\"delete\"}",
        "localhost:4770",
        "auth.AuthService/CheckAccess",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--- ADDRESS ---\nlocalhost:4770"));
    assert!(stdout.contains("--- ENDPOINT ---\nauth.AuthService/CheckAccess"));
    assert!(stdout.contains("--- REQUEST_HEADERS ---\nx-api-key: wrong-key"));
    assert!(stdout.contains("--- REQUEST ---"));
}

#[test]
fn gen_grpcurl_does_not_emit_meta_or_plaintext_option() {
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-plaintext",
        "-d",
        "{\"name\":\"Alex\"}",
        "localhost:4770",
        "helloworld.Greeter/SayHello",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("--- META ---"));
    assert!(!stdout.contains("plaintext: true"));
}

#[test]
fn gen_grpcurl_places_options_after_endpoint() {
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-gzip",
        "-d",
        "{\"name\":\"Alex\"}",
        "localhost:4770",
        "helloworld.Greeter/SayHello",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let endpoint_pos = stdout.find("--- ENDPOINT ---").expect("ENDPOINT section");
    let options_pos = stdout.find("--- OPTIONS ---").expect("OPTIONS section");
    let request_pos = stdout.find("--- REQUEST ---").expect("REQUEST section");

    assert!(endpoint_pos < options_pos);
    assert!(options_pos < request_pos);
    assert!(stdout.contains("compression: gzip"));
}

#[test]
fn gen_grpcurl_writes_to_output_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out_file = dir.path().join("generated.gctf");

    let out = run_cli(&[
        "gen",
        "-o",
        out_file.to_str().expect("utf8 path"),
        "grpcurl",
        "-plaintext",
        "localhost:4770",
        "auth.AuthService/CheckAccess",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).trim().is_empty());

    let generated = std::fs::read_to_string(&out_file).expect("read generated file");
    assert!(generated.contains("--- ADDRESS ---\nlocalhost:4770"));
    assert!(generated.contains("--- ENDPOINT ---\nauth.AuthService/CheckAccess"));
}

#[test]
fn gen_grpcurl_permuted_arguments_generate_stable_output() {
    let a = run_cli(&[
        "gen",
        "grpcurl",
        "-plaintext",
        "-H",
        "x-api-key: wrong-key",
        "-d",
        "{\"action\":\"delete\"}",
        "localhost:4770",
        "auth.AuthService/CheckAccess",
    ]);
    let b = run_cli(&[
        "gen",
        "grpcurl",
        "localhost:4770",
        "-d",
        "{\"action\":\"delete\"}",
        "auth.AuthService/CheckAccess",
        "-H",
        "x-api-key: wrong-key",
        "-plaintext",
    ]);

    assert!(a.status.success());
    assert!(b.status.success());
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn gen_grpcurl_writes_proto_section() {
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-import-path",
        "proto",
        "-import-path",
        "third_party",
        "-proto",
        "a.proto",
        "-proto",
        "b.proto",
        "localhost:4770",
        "auth.AuthService/CheckAccess",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--- PROTO ---"));
    assert!(stdout.contains("files: a.proto,b.proto"));
    assert!(stdout.contains("import_paths: proto,third_party"));
}

/// Extract a `--- NAME ---` section body verbatim (trimmed) for exact
/// content assertions instead of loose substring checks.
fn section_body<'a>(stdout: &'a str, marker: &str) -> &'a str {
    let start = stdout
        .find(marker)
        .unwrap_or_else(|| panic!("missing section {marker:?} in:\n{stdout}"))
        + marker.len();
    let rest = &stdout[start..];
    let end = rest.find("\n--- ").unwrap_or(rest.len());
    rest[..end].trim()
}

#[test]
fn gen_grpcurl_invalid_header_fails() {
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-H",
        "broken_header",
        "localhost:4770",
        "auth.AuthService/CheckAccess",
    ]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid header"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gen_grpcurl_execute_appends_response_section() {
    let address = spawn_health_server().await;

    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-e",
        "-plaintext",
        "-d",
        "{}",
        &address,
        "grpc.health.v1.Health/Check",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        section_body(&stdout, "--- RESPONSE ---"),
        "{\n  \"status\": \"SERVING\"\n}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gen_grpcurl_execute_appends_error_section_on_failure() {
    // Nothing listens on this port — the native client must fail to dial
    // and surface that failure as an ERROR section, not a shell exit code.
    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-e",
        "-plaintext",
        "127.0.0.1:1",
        "grpc.health.v1.Health/Check",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        section_body(&stdout, "--- ERROR ---"),
        "No descriptors loaded via reflection"
    );
}

/// Regression: `tracing` output used to share stdout with a command's actual
/// payload (`src/main.rs`'s subscriber was `.with_writer(std::io::stdout)`),
/// so turning on `RUST_LOG=trace` to debug a failure corrupted the exact
/// `.gctf` a user would redirect into a bug report. Logs must land on
/// stderr; stdout must stay byte-for-byte the same regardless of verbosity.
#[test]
fn gen_grpcurl_verbose_logging_does_not_leak_into_stdout() {
    // `-e` against an unreachable port guarantees at least the dial debug
    // log fires (no live server needed), so this actually exercises the
    // regression instead of trivially comparing two silent runs.
    let args = [
        "gen",
        "grpcurl",
        "-e",
        "-plaintext",
        "127.0.0.1:1",
        "grpc.health.v1.Health/Check",
    ];

    let quiet = support::cli_command()
        .args(args)
        .output()
        .expect("failed to run CLI");

    let verbose = support::cli_command()
        .env("RUST_LOG", "trace")
        .args(args)
        .output()
        .expect("failed to run CLI");

    assert_eq!(
        quiet.stdout, verbose.stdout,
        "stdout must be identical regardless of log verbosity"
    );
    assert!(
        String::from_utf8_lossy(&verbose.stdout).contains("--- ERROR ---"),
        "stdout must still contain the generated document"
    );
    assert!(
        !String::from_utf8_lossy(&verbose.stderr).is_empty(),
        "RUST_LOG=trace must actually produce log output (on stderr)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gen_grpcurl_execute_rejects_unparseable_max_time() {
    let address = spawn_health_server().await;

    let out = run_cli(&[
        "gen",
        "grpcurl",
        "-e",
        "-plaintext",
        "-max-time",
        "not-a-number",
        &address,
        "grpc.health.v1.Health/Check",
    ]);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        section_body(&stdout, "--- ERROR ---"),
        "Invalid -max-time value 'not-a-number'"
    );
}

/// Regression: a negative/zero -max-time parses fine as f64, so it used to
/// slip past the parse-failure check and silently clamp to a 1s timeout
/// instead of being rejected like non-numeric input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gen_grpcurl_execute_rejects_non_positive_max_time() {
    let address = spawn_health_server().await;

    for bad in ["-5", "0", "inf"] {
        let out = run_cli(&[
            "gen",
            "grpcurl",
            "-e",
            "-plaintext",
            "-max-time",
            bad,
            &address,
            "grpc.health.v1.Health/Check",
        ]);

        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            section_body(&stdout, "--- ERROR ---"),
            format!("Invalid -max-time value '{bad}'")
        );
    }
}
