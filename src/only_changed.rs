// Git-diff based file selection for `run --only-changed`. Pure-Rust (gix),
// no shell-out — see memory [[native-zero-dependency]].

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Absolute paths (from `candidates`) whose content differs from their blob
/// in `since`'s tree (default `"HEAD"`), or that aren't present in that tree
/// at all (new/untracked files). Comparison is by content bytes, not commit
/// history — this covers both "what changed since a base branch"
/// (`--since main`) and "what I haven't committed yet" (`--since HEAD`, the
/// default) with one code path.
pub fn changed_files(
    repo_root: &Path,
    since: &str,
    candidates: &[PathBuf],
) -> Result<HashSet<PathBuf>> {
    let repo = gix::discover(repo_root).context("failed to open git repository")?;
    let commit = repo
        .rev_parse_single(since)
        .with_context(|| format!("failed to resolve git ref '{since}'"))?
        .object()
        .with_context(|| format!("failed to resolve git ref '{since}'"))?
        .peel_to_commit()
        .with_context(|| format!("'{since}' does not point at a commit"))?;
    let tree = commit.tree().context("failed to read tree")?;

    let work_dir = repo
        .work_dir()
        .context("repository has no working directory (bare repo?)")?
        .canonicalize()
        .context("failed to canonicalize repository working directory")?;

    let mut changed = HashSet::new();
    for path in candidates {
        // Candidates may be relative (e.g. `./tests/x.gctf`) while `work_dir`
        // is always absolute — canonicalize before stripping the prefix, but
        // keep inserting the original `path` so callers can match it back
        // against their own (possibly relative) file list unchanged.
        let Ok(canonical) = path.canonicalize() else {
            continue; // file gone; nothing to run
        };
        let rel = match canonical.strip_prefix(&work_dir) {
            Ok(rel) => rel,
            Err(_) => {
                // Outside the repo worktree entirely — can't be diffed, treat as changed.
                changed.insert(path.clone());
                continue;
            }
        };
        let current = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => continue, // file gone; nothing to run
        };

        match lookup_blob(&tree, rel) {
            Some(tracked) => {
                if tracked != current {
                    changed.insert(path.clone());
                }
            }
            None => {
                changed.insert(path.clone()); // untracked / new since `since`
            }
        }
    }
    Ok(changed)
}

fn lookup_blob(tree: &gix::Tree<'_>, rel_path: &Path) -> Option<Vec<u8>> {
    let entry = tree.lookup_entry_by_path(rel_path).ok().flatten()?;
    let object = entry.object().ok()?;
    Some(object.data.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Shells out to the real `git` binary — test-harness setup only, never
    /// compiled into the shipped binary (`only_changed::changed_files` above
    /// uses `gix`, not this). Building a real repo through actual `git`
    /// commands is the most trustworthy way to test against real on-disk
    /// git state, short of hand-crafting object files.
    #[cfg(not(miri))]
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

    #[cfg(not(miri))]
    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        dir
    }

    #[test]
    #[cfg(not(miri))]
    fn unmodified_tracked_file_is_not_changed() {
        let dir = init_repo();
        let file = dir.path().join("a.gctf");
        std::fs::write(&file, "content").unwrap();
        git(dir.path(), &["add", "a.gctf"]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);

        let changed = changed_files(dir.path(), "HEAD", &[file]).unwrap();
        assert!(changed.is_empty());
    }

    #[test]
    #[cfg(not(miri))]
    fn modified_tracked_file_is_changed() {
        let dir = init_repo();
        let file = dir.path().join("a.gctf");
        std::fs::write(&file, "content").unwrap();
        git(dir.path(), &["add", "a.gctf"]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);

        std::fs::write(&file, "modified content").unwrap();

        let changed = changed_files(dir.path(), "HEAD", std::slice::from_ref(&file)).unwrap();
        assert_eq!(changed, [file].into_iter().collect());
    }

    #[test]
    #[cfg(not(miri))]
    fn untracked_new_file_is_changed() {
        let dir = init_repo();
        // Need at least one commit for "HEAD" to resolve.
        std::fs::write(dir.path().join("seed.txt"), "seed").unwrap();
        git(dir.path(), &["add", "seed.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);

        let file = dir.path().join("new.gctf");
        std::fs::write(&file, "brand new").unwrap();

        let changed = changed_files(dir.path(), "HEAD", std::slice::from_ref(&file)).unwrap();
        assert_eq!(changed, [file].into_iter().collect());
    }

    #[test]
    #[cfg(not(miri))]
    fn since_a_branch_compares_against_that_branch_not_head() {
        let dir = init_repo();
        let file = dir.path().join("a.gctf");
        std::fs::write(&file, "on main").unwrap();
        git(dir.path(), &["add", "a.gctf"]);
        git(dir.path(), &["commit", "-q", "-m", "on main"]);

        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(&file, "on feature").unwrap();
        git(dir.path(), &["commit", "-q", "-am", "on feature"]);

        // Unchanged relative to the feature branch's own HEAD...
        let vs_head = changed_files(dir.path(), "HEAD", std::slice::from_ref(&file)).unwrap();
        assert!(vs_head.is_empty());

        // ...but changed relative to main.
        let vs_main = changed_files(dir.path(), "main", std::slice::from_ref(&file)).unwrap();
        assert_eq!(vs_main, [file].into_iter().collect());
    }

    #[test]
    #[cfg(not(miri))]
    fn deleted_file_is_silently_skipped_not_an_error() {
        let dir = init_repo();
        std::fs::write(dir.path().join("seed.txt"), "seed").unwrap();
        git(dir.path(), &["add", "seed.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);

        let missing = dir.path().join("gone.gctf");
        let changed = changed_files(dir.path(), "HEAD", &[missing]).unwrap();
        assert!(changed.is_empty());
    }
}
