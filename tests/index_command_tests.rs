#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `index` had no coverage, and it turned out to be broken end-to-end: the
//! recovering parser dropped `BENCH.sources` (its nested YAML list lived on
//! continuation lines that the flat key-value loop discarded), so the command
//! skipped every file with "BENCH.sources is missing or not a YAML list".
//! On top of that, `indexed_by` only accepted a list, while the guide documents
//! a bare scalar for the single-column case.
//!
//! These drive the command the way a user would, from a `.gctf` written the way
//! the documentation shows.

#[path = "support/mod.rs"]
mod support;

/// A bench file plus its CSV. `indexed_by` is written exactly as
/// `docs/guides/bench-sources.md` shows it.
fn fixture(indexed_by: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(
        dir.path().join("data/users.csv"),
        "id,name,status\n1,alice,active\n2,bob,inactive\n3,carol,active\n",
    )
    .unwrap();

    let file = dir.path().join("bench.gctf");
    std::fs::write(
        &file,
        format!(
            "--- ADDRESS ---\nlocalhost:4770\n\n\
             --- ENDPOINT ---\nusers.UserService/GetUser\n\n\
             --- BENCH ---\nmode: fixed\nsources:\n  - name: users\n    file: data/users.csv\n    indexed_by: {indexed_by}\n\n\
             --- REQUEST ---\n{{\n  \"user_id\": \"{{{{users.id}}}}\"\n}}\n"
        ),
    )
    .unwrap();
    (dir, file)
}

#[test]
fn index_builds_an_index_for_a_scalar_indexed_by() {
    let (dir, file) = fixture("id");

    let output = support::cli_command()
        .args(["index", &file.to_string_lossy()])
        .output()
        .expect("failed to run index");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "index must handle the documented scalar `indexed_by`:\n{combined}"
    );
    assert!(
        dir.path().join("data/users.csv.id.gcti").exists(),
        "expected an index file to be written:\n{combined}"
    );
}

#[test]
fn index_builds_an_index_for_a_list_indexed_by() {
    let (dir, file) = fixture("[id]");

    let output = support::cli_command()
        .args(["index", &file.to_string_lossy()])
        .output()
        .expect("failed to run index");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("data/users.csv.id.gcti").exists());
}

/// A second run must reuse the existing index rather than rebuild it, and
/// `--force` must rebuild it anyway.
#[test]
fn index_reuses_then_force_rebuilds() {
    let (dir, file) = fixture("id");
    let index_path = dir.path().join("data/users.csv.id.gcti");

    let first = support::cli_command()
        .args(["index", &file.to_string_lossy()])
        .output()
        .expect("failed to run index");
    assert!(first.status.success());
    assert!(index_path.exists());

    let second = support::cli_command()
        .args(["index", &file.to_string_lossy()])
        .output()
        .expect("failed to run index");
    let out = String::from_utf8_lossy(&second.stderr).into_owned();
    assert!(second.status.success(), "{out}");
    assert!(
        out.contains("Reused: 1") || out.contains("reused=1"),
        "an unchanged source must reuse its index:\n{out}"
    );

    let forced = support::cli_command()
        .args(["index", "--force", &file.to_string_lossy()])
        .output()
        .expect("failed to run index --force");
    let out = String::from_utf8_lossy(&forced.stderr).into_owned();
    assert!(forced.status.success(), "{out}");
    assert!(
        out.contains("Rebuilt: 1") || out.contains("rebuilt=1"),
        "--force must rebuild:\n{out}"
    );
}

/// A file with no BENCH sources is not an error — it is simply skipped.
#[test]
fn index_skips_a_file_without_sources() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\npkg.Svc/Method\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["index", &file.to_string_lossy()])
        .output()
        .expect("failed to run index");

    let out = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        out.contains("Skipped: 1"),
        "a file without BENCH.sources should be skipped, not fail:\n{out}"
    );
}
