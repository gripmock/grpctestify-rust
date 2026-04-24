#![cfg(not(miri))]

use std::path::Path;
use std::process::{Command, Output};

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

fn run_cli(args: &[&str]) -> Output {
    run_cli_internal(args, None)
}

fn run_cli_internal(args: &[&str], path_override: Option<&Path>) -> Output {
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

    if let Some(path) = path_override {
        let current_path = std::env::var("PATH").unwrap_or_default();
        let merged_path = format!("{}:{}", path.display(), current_path);
        cmd.env("PATH", merged_path);
    }

    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("failed to run command")
}

#[cfg(unix)]
fn run_cli_with_path(args: &[&str], path: &Path) -> Output {
    run_cli_internal(args, Some(path))
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("set perms");
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

#[test]
#[cfg(unix)]
fn gen_grpcurl_execute_appends_response_section() {
    let dir = tempfile::tempdir().expect("tempdir");
    let grpcurl = dir.path().join("grpcurl");
    std::fs::write(
        &grpcurl,
        "#!/bin/sh\necho '{\"ok\":true,\"source\":\"fake\"}'\nexit 0\n",
    )
    .expect("write fake grpcurl");
    make_executable(&grpcurl);

    let out = run_cli_with_path(
        &[
            "gen",
            "grpcurl",
            "-e",
            "-plaintext",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ],
        dir.path(),
    );

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--- RESPONSE ---"));
    assert!(stdout.contains("\"ok\": true"));
    assert!(stdout.contains("\"source\": \"fake\""));
}

#[test]
#[cfg(unix)]
fn gen_grpcurl_execute_appends_error_section_on_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let grpcurl = dir.path().join("grpcurl");
    std::fs::write(
        &grpcurl,
        "#!/bin/sh\necho 'permission denied' 1>&2\nexit 7\n",
    )
    .expect("write fake grpcurl");
    make_executable(&grpcurl);

    let out = run_cli_with_path(
        &[
            "gen",
            "grpcurl",
            "-e",
            "localhost:4770",
            "auth.AuthService/CheckAccess",
        ],
        dir.path(),
    );

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--- ERROR ---"));
    assert!(stdout.contains("permission denied"));
}
