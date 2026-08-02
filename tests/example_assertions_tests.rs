#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Proves `examples/assertions/*.gctf` actually run correctly against a
//! real server, the same convention `tests/example_plugins_tests.rs` uses
//! for `examples/plugins/`.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;
use support::spawn_health_server;

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
