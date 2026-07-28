#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Live-network verification of `plugins install`'s real `gix` clone path —
//! DNS, TLS, git smart-HTTP handshake, shallow fetch, checkout. Everything
//! else in this repo's test suite is 100% hermetic/local-only; this can't
//! be, so every test here is `#[ignore]`d and excluded from the default
//! `cargo test --workspace` run. Run manually: `cargo test --test
//! plugins_marketplace_live_tests -- --ignored`.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

/// A real, stable, well-known public repo with no `.rhai` files — the point
/// is proving the clone/checkout mechanics work against a real host, not
/// depending on that repo's content staying a certain way.
#[test]
#[ignore = "requires network access"]
fn install_against_a_real_repo_reaches_the_no_rhai_files_error() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = cli_command()
        .current_dir(dir.path())
        .env("HOME", home.path())
        .args(["plugins", "install", "github.com/octocat/Hello-World"])
        .output()
        .expect("failed to run CLI");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "octocat/Hello-World has no .rhai files, install must fail: {combined}"
    );
    assert!(
        combined.contains("no .rhai files found"),
        "must fail with the specific no-files error, not a network/parse error: {combined}"
    );
}

/// A ref pin that doesn't exist must fail clearly, not hang or panic.
#[test]
#[ignore = "requires network access"]
fn install_with_a_nonexistent_ref_fails_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = cli_command()
        .current_dir(dir.path())
        .env("HOME", home.path())
        .args([
            "plugins",
            "install",
            "github.com/octocat/Hello-World@this-branch-does-not-exist-12345",
        ])
        .output()
        .expect("failed to run CLI");

    assert!(
        !output.status.success(),
        "a nonexistent ref must fail, not silently fall back to something else"
    );
}
