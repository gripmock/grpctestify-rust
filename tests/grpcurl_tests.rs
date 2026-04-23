#![cfg(not(miri))]

use std::path::Path;
use std::process::{Command, Output};

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

fn run_with_optional_runner(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let binary = get_binary();
    let runner = std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER")
        .ok()
        .or_else(|| std::env::var("CROSS_RUNNER").ok());

    let mut cmd = if let Some(runner) = runner {
        let mut parts = runner.split_whitespace();
        let program = parts.next().expect("Runner must not be empty");
        let mut command = Command::new(program);
        command.args(parts);
        command.arg(&binary);
        command
    } else {
        Command::new(&binary)
    };

    cmd.current_dir(cwd).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }

    cmd.output().expect("failed to run grpctestify")
}

fn run_cli_in_dir(cwd: &Path, args: &[&str]) -> Output {
    run_with_optional_runner(cwd, args, &[])
}

fn run_cli_in_dir_with_env(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    run_with_optional_runner(cwd, args, envs)
}

#[test]
fn test_grpcurl_builds_paths_relative_to_invocation_cwd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let tests_dir = root.join("tests");
    let proto_dir = tests_dir.join("proto");
    std::fs::create_dir_all(&proto_dir).expect("mkdirs");

    let gctf_path = tests_dir.join("sample.gctf");
    let gctf = r#"--- ADDRESS ---
localhost:50051

--- ENDPOINT ---
demo.UserService/GetUser

--- PROTO ---
files: proto/user.proto
import_paths: proto

--- REQUEST_HEADERS ---
authorization: Bearer abc

--- REQUEST ---
{"id": 42}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let rel_file = "tests/sample.gctf";
    let out = run_cli_in_dir(root, &["grpcurl", rel_file]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("-proto 'tests/proto/user.proto'"));
    assert!(stdout.contains("-import-path 'tests/proto'"));
    assert!(stdout.contains("'localhost:50051' 'demo.UserService/GetUser'"));
}

#[test]
fn test_grpcurl_json_output_contains_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ADDRESS ---
localhost:50051

--- ENDPOINT ---
demo.UserService/GetUser

--- REQUEST ---
{"id": 42}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf", "--format", "json"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert!(json.get("command").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        json.get("endpoint").and_then(|v| v.as_str()),
        Some("demo.UserService/GetUser")
    );
}

#[test]
fn test_grpcurl_doc_index_selects_single_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("multi.gctf");
    let gctf = r#"--- ADDRESS ---
localhost:50051

--- ENDPOINT ---
demo.UserService/GetUser

--- REQUEST ---
{"id": 1}

--- RESPONSE ---
{"id": 1}

--- ENDPOINT ---
demo.UserService/GetUserV2

--- REQUEST ---
{"id": 2}

--- RESPONSE ---
{"id": 2}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "multi.gctf", "--doc-index", "2"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("GetUserV2"));
    assert!(!stdout.contains("GetUser'"));
}

#[test]
fn test_grpcurl_doc_index_out_of_range_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ENDPOINT ---
demo.UserService/GetUser

--- REQUEST ---
{"id": 42}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf", "--doc-index", "2"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("out of range"));
}

#[test]
fn test_grpcurl_uses_default_address_without_section_or_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ENDPOINT ---
demo.UserService/GetUser

--- REQUEST ---
{"id": 42}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("'localhost:4770'"));
}

#[test]
fn test_grpcurl_uses_env_address_when_section_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ENDPOINT ---
demo.UserService/GetUser

--- REQUEST ---
{"id": 42}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir_with_env(
        root,
        &["grpcurl", "sample.gctf"],
        &[("GRPCTESTIFY_ADDRESS", "https://env.example:7443")],
    );
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("-plaintext"));
    assert!(stdout.contains("'env.example:7443'"));
}

#[test]
fn test_grpcurl_emits_run_parity_tls_compression_and_empty_request() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let tests_dir = root.join("tests");
    std::fs::create_dir_all(tests_dir.join("certs")).expect("mkdirs");

    let gctf_path = tests_dir.join("sample.gctf");
    let gctf = r#"--- ADDRESS ---
https://svc.example:443

--- ENDPOINT ---
demo.UserService/GetUser

--- OPTIONS ---
compression: gzip

--- TLS ---
ca_file: certs/ca.pem
cert_file: certs/client.pem
key_file: certs/client.key
server_name: svc.internal
insecure: true
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "tests/sample.gctf"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("-gzip"));
    assert!(stdout.contains("-cacert 'tests/certs/ca.pem'"));
    assert!(stdout.contains("-cert 'tests/certs/client.pem'"));
    assert!(stdout.contains("-key 'tests/certs/client.key'"));
    assert!(stdout.contains("-servername 'svc.internal'"));
    assert!(stdout.contains("-insecure"));
    assert!(stdout.contains("-d '{}'"));
}

#[test]
fn test_grpcurl_does_not_emit_plaintext_when_tls_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ADDRESS ---
localhost:50051

--- ENDPOINT ---
demo.UserService/GetUser

--- TLS ---
insecure: true

--- REQUEST ---
{"id": 1}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("-plaintext"));
    assert!(stdout.contains("-insecure"));
}

#[test]
fn test_grpcurl_prefers_protoset_over_proto_flags() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ENDPOINT ---
demo.UserService/GetUser

--- PROTO ---
descriptor: api/service.protoset
files: api/service.proto
import_paths: api

--- REQUEST ---
{"id": 1}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("-protoset 'api/service.protoset'"));
    assert!(!stdout.contains("-proto '"));
    assert!(!stdout.contains("-import-path '"));
}

#[test]
fn test_grpcurl_default_includes_all_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gctf_path = root.join("sample.gctf");
    let gctf = r#"--- ENDPOINT ---
demo.UserService/StreamUsers

--- REQUEST ---
{"id": 1}

--- REQUEST ---
{"id": 2}
"#;
    std::fs::write(&gctf_path, gctf).expect("write gctf");

    let out = run_cli_in_dir(root, &["grpcurl", "sample.gctf"]);
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("-d '{\"id\":1}\n{\"id\":2}'"));
}
