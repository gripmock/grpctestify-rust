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
        .workdir()
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
