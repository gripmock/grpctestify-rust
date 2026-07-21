#![cfg(not(miri))]

//! Regression tests for snapshot (`--write`) mode data-loss protection and
//! run-command exit codes.

use std::process::{Command, Output};

fn run_cli(args: &[&str]) -> Output {
    let binary = env!("CARGO_BIN_EXE_grpctestify");
    let runner = std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER")
        .ok()
        .or_else(|| std::env::var("CROSS_RUNNER").ok());

    let mut cmd = if let Some(runner) = runner {
        let mut parts = runner.split_whitespace();
        let program = parts.next().expect("Runner must not be empty");
        let mut command = Command::new(program);
        command.args(parts);
        command.arg(binary);
        command
    } else {
        Command::new(binary)
    };

    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("Failed to execute CLI command")
}

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
    assert!(
        stderr.contains("No test files found"),
        "stderr should explain the empty test set, got:\n{stderr}"
    );
}
