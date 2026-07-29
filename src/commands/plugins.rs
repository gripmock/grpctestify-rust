#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe

use anyhow::{Context, Result, bail};
use apif_plugins::marketplace::{
    self, InstallSource, LockedFile, LockedPlugin, Lockfile, VersionSpec,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::cli::args::{PluginsInstallArgs, PluginsListArgs, PluginsRemoveArgs, PluginsUpdateArgs};

fn target_plugins_dir(global: bool) -> Result<PathBuf> {
    if global {
        apif_plugins::rhai_plugin::user_plugin_dir()
            .context("cannot resolve a user-global plugin directory ($HOME/%USERPROFILE% not set)")
    } else {
        Ok(apif_plugins::rhai_plugin::project_plugin_dir())
    }
}

/// Tag names on a remote, no checkout — resolves `Latest`/`Range` before
/// the real fetch.
fn list_remote_tags(url: &str) -> Result<Vec<String>> {
    let tmp = tempfile::tempdir()?;
    let (repo, _outcome) = gix::prepare_clone(url, tmp.path())?
        .fetch_only(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| anyhow::anyhow!("failed to list tags on {url}: {e}"))?;
    let tags = repo
        .references()?
        .tags()?
        .filter_map(|r| r.ok())
        .filter_map(|r| {
            r.name()
                .as_bstr()
                .to_string()
                .strip_prefix("refs/tags/")
                .map(str::to_string)
        })
        .collect();
    Ok(tags)
}

fn clone_shallow(url: &str, dest: &Path, ref_name: Option<&str>) -> Result<gix::Repository> {
    let mut prep = gix::prepare_clone(url, dest)?
        .with_ref_name(ref_name)
        .map_err(|e| anyhow::anyhow!("invalid ref {ref_name:?}: {e}"))?
        .with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
            1.try_into().expect("1 is nonzero"),
        ));
    let (mut checkout, _fetch_outcome) = prep
        .fetch_then_checkout(gix::progress::Discard, &AtomicBool::new(false))
        .map_err(|e| anyhow::anyhow!("failed to fetch {url} ({ref_name:?}): {e}"))?;
    let (repo, _checkout_outcome) =
        checkout.main_worktree(gix::progress::Discard, &AtomicBool::new(false))?;
    Ok(repo)
}

/// Ref name to fetch (`None` = default branch) plus the matched tag, if any.
fn resolve_ref(source: &InstallSource) -> Result<(Option<String>, Option<String>)> {
    match &source.version {
        VersionSpec::Exact(r) => Ok((Some(r.clone()), None)),
        VersionSpec::Latest => {
            let tags = list_remote_tags(&source.clone_url())?;
            match marketplace::pick_highest_tag(tags.iter().map(String::as_str), None) {
                Some(tag) => Ok((Some(tag.to_string()), Some(tag.to_string()))),
                None => Ok((None, None)),
            }
        }
        VersionSpec::Range(req) => {
            let tags = list_remote_tags(&source.clone_url())?;
            let tag = marketplace::pick_highest_tag(tags.iter().map(String::as_str), Some(req))
                .ok_or_else(|| anyhow::anyhow!("no tag on {} satisfies {req}", source.key()))?;
            Ok((Some(tag.to_string()), Some(tag.to_string())))
        }
    }
}

struct Installed {
    resolved_commit: String,
    resolved_version: Option<String>,
    files: Vec<LockedFile>,
    description: Option<String>,
}

fn install_source(source: &InstallSource, plugins_dir: &Path) -> Result<Installed> {
    let (ref_name, resolved_version) = resolve_ref(source)?;
    let tmp = tempfile::tempdir()?;
    let repo = clone_shallow(&source.clone_url(), tmp.path(), ref_name.as_deref())?;
    let resolved_commit = repo.head_id()?.to_string();

    let work_dir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("clone of {} has no working tree", source.key()))?;
    let source_dir = match &source.subpath {
        Some(sub) => work_dir.join(sub),
        None => work_dir.to_path_buf(),
    };
    if !source_dir.is_dir() {
        bail!(
            "subpath {:?} not found in {}",
            source.subpath.as_deref().unwrap_or(""),
            source.key()
        );
    }

    let (rhai_files, manifest) = resolve_install_files(&source_dir, &source.key())?;

    let dest = marketplace::install_dir(plugins_dir, source);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;

    let mut files = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    for path in &rhai_files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Installed layout is flat (matches the default scan's own
        // behavior) — an explicit `files:` list naming two same-named
        // scripts from different subdirectories would silently overwrite
        // one with the other, so that's a hard error instead.
        if !seen_names.insert(name.clone()) {
            bail!(
                "{}: two listed files both install as {name:?} — installed plugins land in one flat directory, rename one",
                source.key()
            );
        }
        let bytes = std::fs::read(path)?;
        let sha256 = marketplace::sha256_hex(&bytes);
        std::fs::write(dest.join(&name), &bytes)?;
        files.push(LockedFile { path: name, sha256 });
    }

    // Keep the manifest itself alongside the installed scripts so `list`
    // can show the description later without re-cloning the source.
    if manifest.is_some() {
        let manifest_src = source_dir.join(marketplace::MANIFEST_FILE_NAME);
        if let Ok(bytes) = std::fs::read(&manifest_src) {
            let _ = std::fs::write(dest.join(marketplace::MANIFEST_FILE_NAME), bytes);
        }
    }

    Ok(Installed {
        resolved_commit,
        resolved_version,
        files,
        description: manifest.and_then(|m| m.description),
    })
}

/// Whether reading `path` would leave `root`. A symlink is rejected outright
/// rather than resolved: a plugin repo has no legitimate reason to ship one,
/// and `fs::read` follows it.
fn escapes_source(path: &Path, root: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        || !path
            .canonicalize()
            .is_ok_and(|resolved| resolved.starts_with(root))
}

/// Which `.rhai` files to install from a resolved `source_dir`, and the
/// optional manifest that drove the decision. Pure filesystem logic, no git
/// — kept separate from `install_source` so it's unit-testable with a plain
/// tempdir instead of a real clone.
fn resolve_install_files(
    source_dir: &Path,
    source_key: &str,
) -> Result<(Vec<PathBuf>, Option<marketplace::PluginManifest>)> {
    let root = source_dir
        .canonicalize()
        .with_context(|| format!("resolving {}", source_dir.display()))?;

    // Before the manifest is even read: it too is a repo-controlled path, and
    // a parse failure echoes part of the file back in the warning.
    let manifest_path = source_dir.join(marketplace::MANIFEST_FILE_NAME);
    if manifest_path.exists() && escapes_source(&manifest_path, &root) {
        bail!(
            "{source_key}: {} escapes the source directory",
            marketplace::MANIFEST_FILE_NAME
        );
    }

    // Optional repo-side `grpctestify-plugin.yaml`: opt-in, so a present but
    // malformed manifest warns and falls back rather than blocking install —
    // the installing user can't fix another repo's mistake.
    let manifest = match marketplace::read_manifest(source_dir) {
        Ok(m) => m,
        Err(warning) => {
            eprintln!("warning: {source_key}: {warning}");
            None
        }
    };
    if let Some(m) = &manifest
        && let Some(warning) = marketplace::manifest_compat_warning(m, env!("CARGO_PKG_VERSION"))
    {
        eprintln!("warning: {source_key}: {warning}");
    }

    let rhai_files: Vec<PathBuf> = match manifest.as_ref().and_then(|m| m.files.as_ref()) {
        Some(explicit) => {
            let mut paths = Vec::with_capacity(explicit.len());
            for rel in explicit {
                if !rel.ends_with(".rhai") {
                    bail!(
                        "{source_key}: {} lists non-.rhai file {rel:?} in its files:",
                        marketplace::MANIFEST_FILE_NAME
                    );
                }
                // Repo-controlled manifest: reject absolute/`..` entries that escape the source.
                let rel_path = Path::new(rel);
                if rel_path.is_absolute()
                    || rel_path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    bail!(
                        "{source_key}: {} lists {rel:?}, which escapes the source directory",
                        marketplace::MANIFEST_FILE_NAME
                    );
                }
                let path = source_dir.join(rel);
                if !path.is_file() {
                    bail!(
                        "{source_key}: {} lists {rel:?}, but it doesn't exist under {}",
                        marketplace::MANIFEST_FILE_NAME,
                        source_dir.display()
                    );
                }
                paths.push(path);
            }
            paths
        }
        None => {
            let mut found: Vec<PathBuf> = std::fs::read_dir(source_dir)
                .with_context(|| format!("reading {}", source_dir.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rhai"))
                .collect();
            found.sort();
            found
        }
    };

    // Past both arms on purpose: a repo can commit `checks.rhai` as a symlink
    // to `~/.ssh/id_rsa`, and the read below would follow it out of the clone.
    for path in &rhai_files {
        if escapes_source(path, &root) {
            bail!(
                "{source_key}: {:?} escapes the source directory",
                path.strip_prefix(source_dir).unwrap_or(path).display()
            );
        }
    }
    if rhai_files.is_empty() {
        bail!(
            "no .rhai files found in {source_key} ({})",
            source_dir.display()
        );
    }

    Ok((rhai_files, manifest))
}

fn print_summary(action: &str, source: &InstallSource, installed: &Installed) {
    println!("{action} {}", source.key());
    if let Some(d) = &installed.description {
        println!("  {d}");
    }
    println!("  commit:  {}", installed.resolved_commit);
    if let Some(v) = &installed.resolved_version {
        println!("  version: {v}");
    }
    println!("  files:");
    for f in &installed.files {
        println!("    {} ({})", f.path, &f.sha256[..12]);
    }
}

pub fn handle_plugins_install(args: &PluginsInstallArgs) -> Result<()> {
    let source = marketplace::parse_source(&args.source).map_err(anyhow::Error::msg)?;
    let plugins_dir = target_plugins_dir(args.global)?;
    let installed = install_source(&source, &plugins_dir)?;

    let lock_path = marketplace::lockfile_path(&plugins_dir);
    let mut lock = Lockfile::read(&lock_path);
    lock.upsert(LockedPlugin {
        key: source.key(),
        host: source.host.clone(),
        owner: source.owner.clone(),
        repo: source.repo.clone(),
        subpath: source.subpath.clone(),
        requested: raw_spec(&args.source),
        resolved_version: installed.resolved_version.clone(),
        resolved_commit: installed.resolved_commit.clone(),
        installed_at: chrono::Utc::now().to_rfc3339(),
        files: installed.files.clone(),
    });
    lock.write(&lock_path)?;

    print_summary("installed", &source, &installed);
    Ok(())
}

fn raw_spec(source: &str) -> Option<String> {
    source.rsplit_once('@').map(|(_, r)| r.to_string())
}

pub fn handle_plugins_list(args: &PluginsListArgs) -> Result<()> {
    let tiers: Vec<(&str, Result<PathBuf>)> = if args.all {
        vec![
            ("project", target_plugins_dir(false)),
            ("global", target_plugins_dir(true)),
        ]
    } else {
        vec![(
            if args.global { "global" } else { "project" },
            target_plugins_dir(args.global),
        )]
    };

    let mut printed_any = false;
    for (label, dir) in tiers {
        let Ok(dir) = dir else { continue };
        let lock = Lockfile::read(&marketplace::lockfile_path(&dir));
        if lock.plugins.is_empty() {
            continue;
        }
        printed_any = true;
        println!("{label} ({}):", dir.display());
        for p in &lock.plugins {
            let version = p.resolved_version.as_deref().unwrap_or("-");
            println!(
                "  {}  {}  {}  ({} files)",
                p.key,
                version,
                &p.resolved_commit[..12.min(p.resolved_commit.len())],
                p.files.len()
            );
            // The manifest (if the source had one) was copied alongside the
            // installed scripts at install time specifically so `list` can
            // show its description without re-cloning the source.
            let install_dir = dir
                .join("installed")
                .join(&p.host)
                .join(&p.owner)
                .join(&p.repo);
            if let Ok(Some(m)) = marketplace::read_manifest(&install_dir)
                && let Some(d) = m.description
            {
                println!("      {d}");
            }
        }
    }
    if !printed_any {
        println!("No installed plugins.");
    }
    Ok(())
}

pub fn handle_plugins_remove(args: &PluginsRemoveArgs) -> Result<()> {
    let plugins_dir = target_plugins_dir(args.global)?;
    let lock_path = marketplace::lockfile_path(&plugins_dir);
    let mut lock = Lockfile::read(&lock_path);
    let Some(entry) = lock.get(&args.name).cloned() else {
        bail!("{} is not installed", args.name);
    };
    // The lockfile ships with the repo: a `host: "../../../victim"` entry
    // turned this into `rm -rf` on an arbitrary directory.
    let installed_root = plugins_dir.join("installed");
    let source =
        marketplace::parse_source(&format!("{}/{}/{}", entry.host, entry.owner, entry.repo))
            .map_err(|e| anyhow::anyhow!("refusing to remove {}: {e}", args.name))?;
    let dir = installed_root
        .join(&source.host)
        .join(&source.owner)
        .join(&source.repo);
    if dir.exists() {
        let (Ok(dir_canon), Ok(root_canon)) = (dir.canonicalize(), installed_root.canonicalize())
        else {
            bail!("refusing to remove {}: unresolvable plugin path", args.name);
        };
        if !dir_canon.starts_with(&root_canon) {
            bail!(
                "refusing to remove {}: path escapes the plugin dir",
                args.name
            );
        }
        std::fs::remove_dir_all(&dir)?;
    }
    lock.remove(&args.name);
    lock.write(&lock_path)?;
    println!("removed {}", args.name);
    Ok(())
}

pub fn handle_plugins_update(args: &PluginsUpdateArgs) -> Result<()> {
    let plugins_dir = target_plugins_dir(args.global)?;
    let lock_path = marketplace::lockfile_path(&plugins_dir);
    let mut lock = Lockfile::read(&lock_path);

    let keys: Vec<String> = match &args.name {
        Some(name) => vec![name.clone()],
        None => lock.plugins.iter().map(|p| p.key.clone()).collect(),
    };
    if keys.is_empty() {
        println!("No installed plugins.");
        return Ok(());
    }

    for key in keys {
        let Some(entry) = lock.get(&key).cloned() else {
            bail!("{key} is not installed");
        };
        let mut source_str = format!("{}/{}/{}", entry.host, entry.owner, entry.repo);
        if let Some(sub) = &entry.subpath {
            source_str.push('/');
            source_str.push_str(sub);
        }
        if let Some(spec) = &entry.requested {
            source_str.push('@');
            source_str.push_str(spec);
        }
        let source = marketplace::parse_source(&source_str).map_err(anyhow::Error::msg)?;
        let installed = install_source(&source, &plugins_dir)?;

        if installed.resolved_commit == entry.resolved_commit {
            println!(
                "{key} already up to date ({})",
                &entry.resolved_commit[..12.min(entry.resolved_commit.len())]
            );
            continue;
        }

        lock.upsert(LockedPlugin {
            key: source.key(),
            host: source.host.clone(),
            owner: source.owner.clone(),
            repo: source.repo.clone(),
            subpath: source.subpath.clone(),
            requested: entry.requested.clone(),
            resolved_version: installed.resolved_version.clone(),
            resolved_commit: installed.resolved_commit.clone(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            files: installed.files.clone(),
        });
        print_summary("updated", &source, &installed);
    }

    lock.write(&lock_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(miri, ignore)]
    // Regression: the guard sat inside the manifest arm only, so the default
    // no-manifest scan read straight through a symlink.
    #[cfg(unix)]
    #[test]
    fn resolve_install_files_rejects_a_symlink_out_of_the_source_dir() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("id_rsa");
        std::fs::write(&secret, "PRIVATE KEY").unwrap();
        let source = dir.path().join("repo");
        std::fs::create_dir_all(&source).unwrap();
        std::os::unix::fs::symlink(&secret, source.join("checks.rhai")).unwrap();

        // No manifest: the default directory-scan arm.
        let err = resolve_install_files(&source, "test/repo").unwrap_err();
        assert!(
            err.to_string().contains("escapes the source directory"),
            "unexpected error: {err}"
        );

        // And with a manifest listing it explicitly.
        std::fs::write(
            source.join(marketplace::MANIFEST_FILE_NAME),
            "name: t\nfiles:\n  - checks.rhai\n",
        )
        .unwrap();
        let err = resolve_install_files(&source, "test/repo").unwrap_err();
        assert!(
            err.to_string().contains("escapes the source directory"),
            "unexpected error: {err}"
        );
    }

    // The manifest is a repo-controlled path too, and a parse failure echoes
    // part of the file back in the warning.
    #[cfg(unix)]
    #[test]
    fn resolve_install_files_rejects_a_symlinked_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("secret.yaml");
        std::fs::write(&secret, "name: leaked\n").unwrap();
        let source = dir.path().join("repo");
        std::fs::create_dir_all(&source).unwrap();
        std::os::unix::fs::symlink(&secret, source.join(marketplace::MANIFEST_FILE_NAME)).unwrap();

        let err = resolve_install_files(&source, "test/repo").unwrap_err();
        assert!(
            err.to_string().contains("escapes the source directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_install_files_default_scan_finds_rhai_files_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.rhai"), "").unwrap();
        std::fs::write(dir.path().join("a.rhai"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();

        let (files, manifest) = resolve_install_files(dir.path(), "test/repo").unwrap();
        assert!(manifest.is_none());
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.rhai", "b.rhai"]);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_errors_when_none_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_install_files(dir.path(), "test/repo").unwrap_err();
        assert!(err.to_string().contains("no .rhai files found"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_manifest_explicit_list_overrides_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ignored.rhai"), "").unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/util.rhai"), "").unwrap();
        std::fs::write(
            dir.path().join(marketplace::MANIFEST_FILE_NAME),
            "description: A test plugin\nfiles:\n  - nested/util.rhai\n",
        )
        .unwrap();

        let (files, manifest) = resolve_install_files(dir.path(), "test/repo").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap().to_string_lossy(), "util.rhai");
        assert_eq!(
            manifest.unwrap().description.as_deref(),
            Some("A test plugin")
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_manifest_missing_listed_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(marketplace::MANIFEST_FILE_NAME),
            "files:\n  - does_not_exist.rhai\n",
        )
        .unwrap();

        let err = resolve_install_files(dir.path(), "test/repo").unwrap_err();
        assert!(err.to_string().contains("does_not_exist.rhai"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_manifest_non_rhai_listed_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("script.txt"), "").unwrap();
        std::fs::write(
            dir.path().join(marketplace::MANIFEST_FILE_NAME),
            "files:\n  - script.txt\n",
        )
        .unwrap();

        let err = resolve_install_files(dir.path(), "test/repo").unwrap_err();
        assert!(err.to_string().contains("non-.rhai"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_manifest_escaping_path_is_rejected() {
        // A repo-controlled `files:` entry with `..` must not read/install a
        // file from outside the cloned source directory.
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("pkg");
        std::fs::create_dir(&sub).unwrap();
        // A real .rhai sitting outside the source dir — the target of the escape.
        std::fs::write(dir.path().join("outside.rhai"), "").unwrap();
        std::fs::write(
            sub.join(marketplace::MANIFEST_FILE_NAME),
            "files:\n  - ../outside.rhai\n",
        )
        .unwrap();

        let err = resolve_install_files(&sub, "test/repo").unwrap_err();
        assert!(
            err.to_string().contains("escapes the source directory"),
            "expected traversal rejection, got: {err}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn resolve_install_files_malformed_manifest_warns_and_falls_back_to_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rhai"), "").unwrap();
        std::fs::write(
            dir.path().join(marketplace::MANIFEST_FILE_NAME),
            "not: [valid: yaml",
        )
        .unwrap();

        // Malformed manifest must not block install — falls back to the
        // default scan, same as if no manifest existed at all.
        let (files, manifest) = resolve_install_files(dir.path(), "test/repo").unwrap();
        assert!(manifest.is_none());
        assert_eq!(files.len(), 1);
    }
}
