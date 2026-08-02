#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
//! Shared CLI-invocation helpers for integration tests. A directory module
//! (`tests/support/mod.rs`), not its own test binary — pull it in with
//! `#[path = "support/mod.rs"] mod support;` (or `mod support;` if the crate
//! root re-exports it) and `use support::{run_cli, fixture_path};`.

#![allow(dead_code)]

use std::process::{Command, Output};

pub fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

pub fn fixture_path(rel: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

/// A `Command` already pointed at the compiled `grpctestify` binary,
/// honouring cross-compile CI runners
/// (`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER` / `CROSS_RUNNER`) so tests
/// still work when the test host can't execute the target binary directly
/// (e.g. running an aarch64 binary under QEMU in CI). Callers add
/// args/cwd/env before running it — this is the shared primitive every
/// per-file `run_cli`-style variant (in a different dir, with extra env,
/// with a path override, ...) should build on instead of re-deriving.
pub fn cli_command() -> Command {
    let binary = get_binary();
    let runner = std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER")
        .ok()
        .or_else(|| std::env::var("CROSS_RUNNER").ok());

    if let Some(runner) = runner {
        let mut parts = runner.split_whitespace();
        let program = parts.next().expect("runner must not be empty");
        let mut command = Command::new(program);
        command.args(parts).arg(&binary);
        command
    } else {
        Command::new(&binary)
    }
}

/// Run the compiled binary with `args` from the repo root.
pub fn run_cli(args: &[&str]) -> Output {
    cli_command()
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("failed to execute CLI command")
}

pub fn parse_json_stdout(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "CLI failed with status {:?}\nstderr:\n{}\nstdout:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "Invalid JSON output: {e}\nstderr:\n{}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Like [`parse_json_stdout`], but doesn't require a zero exit status —
/// for commands that intentionally exit non-zero while still emitting a
/// structured JSON error/report body.
pub fn parse_json_stdout_any_status(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "Invalid JSON output: {e}\nstderr:\n{}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Spawn a `grpc.health.v1.Health` server (SERVING) plus reflection on an
/// ephemeral port; returns the `host:port` to dial. The one shared copy —
/// every integration test that needs a live health target uses this instead
/// of re-pasting the tonic setup. Tests that must hold the reporter handle to
/// flip status mid-run keep their own tuple-returning variant.
pub async fn spawn_health_server() -> String {
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

/// Run the CLI in `dir` as an isolated, trusted-plugins invocation: `dir` is
/// both `$HOME` and the working directory, and script plugins are pre-approved
/// (`GRPCTESTIFY_TRUST_PLUGINS=1`) since a non-interactive test can't answer
/// the trust prompt. The shared version of the per-file `run_in`/`run_isolated`
/// copies.
pub fn run_isolated(dir: &std::path::Path, args: &[&str]) -> Output {
    cli_command()
        .current_dir(dir)
        .env("HOME", dir)
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(args)
        .output()
        .expect("failed to run CLI")
}

/// Compare `actual` against `tests/golden/<rel>`, where `rel` includes the
/// `.golden` suffix and any subdirectory (`"output_forms/run_json.golden"`).
///
/// `UPDATE_GOLDEN=1` rewrites the file instead of asserting — how every golden
/// in this repo is (re)generated. `DEBUG_GOLDEN=1` additionally dumps the
/// actual output next to the temp dir for eyeballing a mismatch.
pub fn assert_golden(rel: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(rel);

    if std::env::var("DEBUG_GOLDEN").is_ok() {
        let dump = std::env::temp_dir().join(format!("{}.actual", rel.replace('/', "_")));
        std::fs::write(&dump, actual).expect("write DEBUG_GOLDEN dump");
        eprintln!("DEBUG_GOLDEN: actual output written to {}", dump.display());
    }

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().expect("golden has a parent"))
            .expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden file {}: {e}\nrun with UPDATE_GOLDEN=1 to create it\nactual output was:\n{actual}",
            path.display()
        )
    });
    assert_eq!(
        expected, actual,
        "golden mismatch for '{rel}' (rerun with UPDATE_GOLDEN=1 if this change is intentional)"
    );
}

/// Every ```gctf snippet in the repo's markdown, as `(file, line, body)`.
///
/// Covers `docs/`, the root and per-crate READMEs, and `examples/` — the
/// generated `docs/.vitepress` output is skipped. Shared so that tests
/// checking different properties of these snippets provably look at the same
/// set: the two callers previously walked different trees.
pub fn markdown_gctf_blocks() -> Vec<(std::path::PathBuf, usize, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == ".vitepress") {
                    continue;
                }
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("docs"), &mut files);
    walk(&root.join("crates"), &mut files);
    walk(&root.join("examples"), &mut files);
    files.push(root.join("README.md"));
    files.retain(|p| p.exists());
    files.sort();

    let mut blocks = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).expect("read markdown");
        let mut rest = text.as_str();
        let mut consumed = 0usize;
        while let Some(start) = rest.find("```gctf\n") {
            let body_at = start + "```gctf\n".len();
            let Some(end_rel) = rest[body_at..].find("```") else {
                break;
            };
            let line = text[..consumed + body_at].matches('\n').count() + 1;
            blocks.push((
                path.clone(),
                line,
                rest[body_at..body_at + end_rel].to_string(),
            ));
            let advance = body_at + end_rel + 3;
            consumed += advance;
            rest = &rest[advance..];
        }
    }
    blocks
}
