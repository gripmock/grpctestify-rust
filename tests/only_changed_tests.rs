#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

use std::path::Path;
use std::process::Command;

/// Shells out to the real `git` binary — test-harness setup only, mirroring
/// `src/only_changed.rs`'s own test helper (the shipped `--only-changed`
/// implementation uses `gix`, never a `git` subprocess).
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed");
}

const UNARY: &str =
    "--- ENDPOINT ---\nsvc.Thing/Do\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";

#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn run_only_changed_dry_run_skips_unmodified_files() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.gctf"), UNARY).unwrap();
    std::fs::write(dir.path().join("b.gctf"), UNARY).unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);

    // Modify only b.gctf.
    std::fs::write(dir.path().join("b.gctf"), format!("{UNARY}\n# touched\n")).unwrap();

    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed", "--dry-run"])
        .output()
        .expect("failed to run CLI");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("b.gctf"), "stdout: {stdout}");
    assert!(!stdout.contains("a.gctf"), "stdout: {stdout}");
}

#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn run_only_changed_since_a_branch() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.gctf"), UNARY).unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "on main"]);

    git(dir.path(), &["checkout", "-q", "-b", "feature"]);
    std::fs::write(
        dir.path().join("a.gctf"),
        format!("{UNARY}\n# feature change\n"),
    )
    .unwrap();
    git(dir.path(), &["commit", "-q", "-am", "on feature"]);

    // Nothing changed relative to the feature branch's own HEAD.
    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed", "--dry-run"])
        .output()
        .expect("failed to run CLI");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("No test files found")
            || String::from_utf8_lossy(&output.stderr).contains("No test files found"),
        "expected nothing changed vs HEAD: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Changed relative to main.
    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed", "--since", "main", "--dry-run"])
        .output()
        .expect("failed to run CLI");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("a.gctf"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
