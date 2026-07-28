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
