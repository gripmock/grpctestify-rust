#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Regression tests for snapshot (`--write`) mode data-loss protection and
//! run-command exit codes.

#[path = "support/mod.rs"]
mod support;
use support::run_cli;

/// `run --write` against a down server must fail and must NOT rewrite the
/// test file (previously it emptied the RESPONSE section and exited 0).
#[test]
fn write_mode_down_server_keeps_file_and_fails() {
    let dir = tempfile::tempdir().unwrap();

    // Proto next to the test file so relative PROTO paths resolve.
    let proto_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/e2e/examples/helloworld/helloworld.proto");
    std::fs::copy(&proto_src, dir.path().join("helloworld.proto")).unwrap();

    // Pick a port with nothing listening on it.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let content = format!(
        "--- ADDRESS ---\nlocalhost:{port}\n\n--- ENDPOINT ---\nhelloworld.Greeter/SayHello\n\n--- PROTO ---\nfiles: helloworld.proto\nimport_paths: .\n\n--- REQUEST ---\n{{\n  \"name\": \"World\"\n}}\n\n--- RESPONSE ---\n{{\n  \"message\": \"Hello World\"\n}}\n"
    );
    let test_path = dir.path().join("down_server.gctf");
    std::fs::write(&test_path, &content).unwrap();

    let output = run_cli(&["run", "--write", test_path.to_str().unwrap()]);

    assert!(
        !output.status.success(),
        "write mode against a down server must exit non-zero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let after = std::fs::read_to_string(&test_path).unwrap();
    assert_eq!(
        after, content,
        "snapshot file must not be modified when the server is unreachable"
    );
}

/// An empty (or fully filtered) test set must exit non-zero so CI cannot
/// silently pass on a path or --tags typo.
#[test]
fn empty_test_set_exits_non_zero() {
    let dir = tempfile::tempdir().unwrap();

    let output = run_cli(&["run", dir.path().to_str().unwrap()]);

    assert!(
        !output.status.success(),
        "empty test set must exit non-zero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Regression: this used to print via both `warn!()` (stdout) and a
    // hand-written `eprintln!` (stderr) with near-identical text — two
    // near-duplicate warnings on a merged terminal.
    assert_eq!(
        stderr.matches("No test files found").count(),
        1,
        "warning must appear exactly once, got:\n{stderr}"
    );
    assert!(
        stderr.contains("No test files found"),
        "stderr should explain the empty test set, got:\n{stderr}"
    );
}
