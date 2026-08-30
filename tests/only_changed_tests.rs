#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

use std::path::Path;

/// Real on-disk git repo via `gix` — no shell-out, see memory
/// [[native-zero-dependency]]. Sets the identity needed to write commits.
fn init_repo(dir: &Path) -> gix::Repository {
    let mut repo = gix::init(dir).unwrap();
    let mut cfg = repo.config_snapshot_mut();
    cfg.set_raw_value(gix::config::tree::User::NAME, "test")
        .unwrap();
    cfg.set_raw_value(gix::config::tree::User::EMAIL, "test@example.com")
        .unwrap();
    drop(cfg);
    repo
}

/// Force HEAD to `refs/heads/main` regardless of this machine's
/// `init.defaultBranch` config, so `--since main` below is deterministic
/// across environments.
fn point_head_at_main(repo: &gix::Repository) {
    let name: gix::refs::FullName = "refs/heads/main".try_into().unwrap();
    repo.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: Default::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Symbolic(name),
        },
        name: "HEAD".try_into().unwrap(),
        deref: false,
    })
    .unwrap();
}

/// Detach HEAD at `commit_id` — further commits move HEAD only, leaving
/// whatever branch it used to point at frozen where it was.
fn detach_head(repo: &gix::Repository, commit_id: gix::ObjectId) {
    repo.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: Default::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Object(commit_id),
        },
        name: "HEAD".try_into().unwrap(),
        deref: false,
    })
    .unwrap();
}

/// Write `files` to disk under `dir` and commit them as a new commit on top of `parent`.
fn write_commit(
    repo: &gix::Repository,
    dir: &Path,
    parent: Option<gix::ObjectId>,
    files: &[(&str, &str)],
    message: &str,
) -> gix::ObjectId {
    let entries = files
        .iter()
        .map(|(name, content)| {
            std::fs::write(dir.join(name), content).unwrap();
            gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: (*name).into(),
                oid: repo.write_blob(content.as_bytes()).unwrap().detach(),
            }
        })
        .collect();
    let tree_id = repo
        .write_object(&gix::objs::Tree { entries })
        .unwrap()
        .detach();
    repo.commit("HEAD", message, tree_id, parent)
        .unwrap()
        .detach()
}

const UNARY: &str =
    "--- ENDPOINT ---\nsvc.Thing/Do\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";

#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn run_only_changed_dry_run_skips_unmodified_files() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    write_commit(
        &repo,
        dir.path(),
        None,
        &[("a.gctf", UNARY), ("b.gctf", UNARY)],
        "init",
    );

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

/// A branch that touched no test file is the ordinary case this flag exists
/// for; it reached the same "no test files found" as a typo'd path and failed
/// the build.
#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn a_run_with_nothing_changed_is_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    write_commit(
        &repo,
        dir.path(),
        None,
        &[
            ("a.gctf", UNARY),
            (
                "b.httf",
                "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /x\n\n--- ASSERTS ---\n@status() == 200\n",
            ),
        ],
        "init",
    );

    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed"])
        .output()
        .expect("failed to run CLI");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "stdout: {stdout}");
    assert!(stdout.contains("no test file changed"), "stdout: {stdout}");
}

/// A path that matches nothing is still a mistake, flag or no flag.
#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn a_path_that_matches_nothing_still_fails() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());

    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed"])
        .output()
        .expect("failed to run CLI");

    assert!(!output.status.success());
}

#[cfg_attr(miri, ignore)]
#[test]
#[cfg(not(miri))]
fn run_only_changed_since_a_branch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    point_head_at_main(&repo);
    let on_main = write_commit(&repo, dir.path(), None, &[("a.gctf", UNARY)], "on main");

    // Diverge from main without moving it, like being on a feature branch.
    detach_head(&repo, on_main);
    write_commit(
        &repo,
        dir.path(),
        Some(on_main),
        &[("a.gctf", &format!("{UNARY}\n# feature change\n"))],
        "on feature",
    );

    // Nothing changed relative to the feature branch's own HEAD.
    let output = cli_command()
        .current_dir(dir.path())
        .args(["run", ".", "--only-changed", "--dry-run"])
        .output()
        .expect("failed to run CLI");
    /* Nothing changed is not "nothing found": the run says so and succeeds. */
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no test file changed"),
        "expected nothing changed vs HEAD: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success());

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
