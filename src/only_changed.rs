use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        let rel = match canonical.strip_prefix(&work_dir) {
            Ok(rel) => rel,
            Err(_) => {
                changed.insert(path.clone());
                continue;
            }
        };
        let current = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };

        match lookup_blob(&tree, rel) {
            Some(tracked) => {
                if tracked != current {
                    changed.insert(path.clone());
                }
            }
            None => {
                changed.insert(path.clone());
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
