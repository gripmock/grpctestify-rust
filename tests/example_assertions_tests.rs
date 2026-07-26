#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Proves `examples/assertions/*.gctf` actually run correctly against a
//! real server, the same convention `tests/example_plugins_tests.rs` uses
//! for `examples/plugins/`.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jq_pipelines_example_passes_against_a_real_server() {
    let address = spawn_health_server().await;
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/assertions/jq-pipelines.gctf"),
    )
    .expect("read jq-pipelines.gctf");
    let content = source.replace("localhost:50051", &address);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("jq-pipelines.gctf");
    std::fs::write(&file, content).unwrap();

    let output = cli_command()
        .args(["run", "--verbose", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
