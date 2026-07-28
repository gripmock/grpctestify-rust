//! `plugins install` parsing, lockfile, checksums. No network here — the
//! main crate drives the `gix` clone and calls back into this module.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// A bare version (`v1.2.3`) is `Exact`, matched literally — only an
/// explicit range operator (`^`/`~`/...) is `Range`. Not npm's
/// bare-means-caret: there's no registry here to make that assumption safe.
#[derive(Debug, Clone, PartialEq)]
pub enum VersionSpec {
    Latest,
    Exact(String),
    Range(semver::VersionReq),
}

const RANGE_OPERATOR_PREFIXES: [&str; 6] = ["^", "~", ">", "<", "=", ","];

fn parse_version_spec(spec: Option<&str>) -> Result<VersionSpec, String> {
    let Some(spec) = spec else {
        return Ok(VersionSpec::Latest);
    };
    if RANGE_OPERATOR_PREFIXES.iter().any(|p| spec.starts_with(p)) {
        return semver::VersionReq::parse(spec)
            .map(VersionSpec::Range)
            .map_err(|e| format!("invalid semver range {spec:?}: {e}"));
    }
    Ok(VersionSpec::Exact(spec.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallSource {
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub subpath: Option<String>,
    pub version: VersionSpec,
}

impl InstallSource {
    pub fn clone_url(&self) -> String {
        format!("https://{}/{}/{}.git", self.host, self.owner, self.repo)
    }

    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo)
    }
}

/// `host/owner/repo[/subpath...][@spec]`, e.g. `github.com/owner/repo`,
/// `gitlab.com/owner/repo@v1.2.0`, `github.com/owner/repo@^1.2.0`.
pub fn parse_source(input: &str) -> Result<InstallSource, String> {
    let s = input
        .trim()
        .strip_prefix("https://")
        .unwrap_or(input.trim());
    if s.is_empty() {
        return Err("empty plugin source".to_string());
    }

    let (path, spec) = match s.rsplit_once('@') {
        Some((p, r)) if !p.is_empty() && !r.is_empty() => (p, Some(r)),
        _ => (s, None),
    };
    let version = parse_version_spec(spec)?;

    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let host = parts.next().ok_or("expected host/owner/repo")?;
    let owner = parts.next().ok_or("expected host/owner/repo")?;
    let repo = parts
        .next()
        .ok_or("expected host/owner/repo")?
        .trim_end_matches(".git");
    let rest: Vec<&str> = parts.collect();
    let subpath = (!rest.is_empty()).then(|| rest.join("/"));

    if host.is_empty() || owner.is_empty() || repo.is_empty() {
        return Err("expected host/owner/repo".to_string());
    }

    // Reject `..`/`.`/backslash: each segment is later joined onto a local dir.
    for segment in [host, owner, repo].into_iter().chain(rest.iter().copied()) {
        if segment == ".." || segment == "." || segment.contains('\\') {
            return Err(format!("invalid path segment {segment:?} in plugin source"));
        }
    }

    Ok(InstallSource {
        host: host.to_string(),
        owner: owner.to_string(),
        repo: repo.to_string(),
        subpath,
        version,
    })
}

/// `None` for a non-version tag name.
pub fn parse_semver_tag(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Highest tag matching `req` (or highest overall if `req` is `None`).
pub fn pick_highest_tag<'a>(
    tags: impl IntoIterator<Item = &'a str>,
    req: Option<&semver::VersionReq>,
) -> Option<&'a str> {
    tags.into_iter()
        .filter_map(|tag| parse_semver_tag(tag).map(|v| (tag, v)))
        .filter(|(_, v)| req.is_none_or(|r| r.matches(v)))
        .max_by(|(_, a), (_, b)| a.cmp(b))
        .map(|(tag, _)| tag)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `<plugins_dir>/installed/<host>/<owner>/<repo>` — where an installed
/// source's `.rhai` files land, kept apart from hand-authored scripts
/// directly under `<plugins_dir>`.
pub fn install_dir(plugins_dir: &Path, source: &InstallSource) -> PathBuf {
    plugins_dir
        .join("installed")
        .join(&source.host)
        .join(&source.owner)
        .join(&source.repo)
}

/// `<plugins_dir>/../plugins.lock.yaml` — a sibling of `plugins/` itself,
/// not under it (so it's never mistaken for a `.rhai` script).
pub fn lockfile_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir
        .parent()
        .map(|p| p.join("plugins.lock.yaml"))
        .unwrap_or_else(|| plugins_dir.join("plugins.lock.yaml"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPlugin {
    pub key: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    /// The raw `@spec` the user typed (`None` if omitted — a `Latest` install).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    /// The concrete tag matched, if `requested` was a range or absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
    pub resolved_commit: String,
    pub installed_at: String,
    pub files: Vec<LockedFile>,
}

/// Optional repo-side enrichment, same trust posture as the `@pure` doc-tags
/// convention: additive/opt-in, authored by the plugin repo (not the
/// installing user), never hard-required. No `version:` field — the git
/// tag/commit already is the version (see the proposal's "the tag IS the
/// version", adopted from Go modules).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct PluginManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `grpctestify` compat range (`semver::VersionReq` syntax, e.g. `">=1.8"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpctestify: Option<String>,
    /// Explicit `.rhai` file list (paths relative to the resolved subpath),
    /// overriding the default non-recursive same-directory `.rhai` scan —
    /// lets a repo point at nested files or exclude ones that shouldn't ship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

pub const MANIFEST_FILE_NAME: &str = "grpctestify-plugin.yaml";

/// Read `grpctestify-plugin.yaml` from `source_dir`, if present. `Ok(None)`
/// means no manifest file exists — not an error, the whole thing is opt-in.
/// `Err` is a *present but unparseable* manifest; the caller should warn and
/// fall back to the default scan rather than hard-fail the install — a repo
/// author's mistake in this optional file shouldn't block the installing
/// user, who can't fix someone else's manifest anyway.
pub fn read_manifest(source_dir: &Path) -> Result<Option<PluginManifest>, String> {
    let path = source_dir.join(MANIFEST_FILE_NAME);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    serde_yaml_ng::from_str(&content)
        .map(Some)
        .map_err(|e| format!("{MANIFEST_FILE_NAME} is present but malformed, ignoring it: {e}"))
}

/// `None` when there's nothing to warn about: no `grpctestify:` constraint
/// declared, or the constraint/running version don't even parse as semver
/// (never blocks — the manifest is informational, not enforced).
pub fn manifest_compat_warning(manifest: &PluginManifest, running_version: &str) -> Option<String> {
    let range = manifest.grpctestify.as_deref()?;
    let req = semver::VersionReq::parse(range).ok()?;
    let version = semver::Version::parse(running_version).ok()?;
    if req.matches(&version) {
        None
    } else {
        Some(format!(
            "declares grpctestify compat range {range:?}, this is {running_version} — it may not work as expected"
        ))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default)]
    pub plugins: Vec<LockedPlugin>,
}

impl Lockfile {
    pub fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_yaml_ng::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml_ng::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, yaml)
    }

    pub fn upsert(&mut self, entry: LockedPlugin) {
        self.plugins.retain(|p| p.key != entry.key);
        self.plugins.push(entry);
        self.plugins.sort_by(|a, b| a.key.cmp(&b.key));
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.plugins.len();
        self.plugins.retain(|p| p.key != key);
        self.plugins.len() != before
    }

    pub fn get(&self, key: &str) -> Option<&LockedPlugin> {
        self.plugins.iter().find(|p| p.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_host_owner_repo() {
        let s = parse_source("github.com/owner/repo").unwrap();
        assert_eq!(s.host, "github.com");
        assert_eq!(s.owner, "owner");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.subpath, None);
        assert_eq!(s.version, VersionSpec::Latest);
        assert_eq!(s.clone_url(), "https://github.com/owner/repo.git");
        assert_eq!(s.key(), "github.com/owner/repo");
    }

    #[test]
    fn parses_https_prefix_and_dot_git_suffix() {
        let s = parse_source("https://gitlab.com/owner/repo.git").unwrap();
        assert_eq!(s.host, "gitlab.com");
        assert_eq!(s.repo, "repo");
    }

    #[test]
    fn bare_version_pin_is_exact_not_a_caret_range() {
        let s = parse_source("github.com/owner/repo@v1.2.3").unwrap();
        assert_eq!(s.repo, "repo");
        assert_eq!(s.version, VersionSpec::Exact("v1.2.3".to_string()));
    }

    #[test]
    fn branch_name_pin_is_exact() {
        let s = parse_source("github.com/owner/repo@main").unwrap();
        assert_eq!(s.version, VersionSpec::Exact("main".to_string()));
    }

    #[test]
    fn caret_prefixed_spec_is_a_semver_range() {
        let s = parse_source("github.com/owner/repo@^1.2.0").unwrap();
        assert_eq!(
            s.version,
            VersionSpec::Range(semver::VersionReq::parse("^1.2.0").unwrap())
        );
    }

    #[test]
    fn invalid_range_spec_is_a_clear_error() {
        let err = parse_source("github.com/owner/repo@^not-a-version").unwrap_err();
        assert!(err.contains("invalid semver range"), "{err}");
    }

    #[test]
    fn parses_subpath_and_ref_together() {
        let s = parse_source("github.com/owner/repo/plugins/dir@main").unwrap();
        assert_eq!(s.subpath, Some("plugins/dir".to_string()));
        assert_eq!(s.version, VersionSpec::Exact("main".to_string()));
    }

    #[test]
    fn rejects_missing_repo() {
        assert!(parse_source("github.com/owner").is_err());
    }

    #[test]
    fn rejects_path_traversal_segments() {
        // `..` in any position would let a crafted source escape the local
        // install dir / clone working tree once joined.
        assert!(parse_source("github.com/owner/repo/../../etc/passwd").is_err());
        assert!(parse_source("github.com/../repo/x").is_err());
        assert!(parse_source("github.com/owner/repo/sub/..@v1").is_err());
        assert!(parse_source("github.com/owner/repo/a\\b").is_err());
        // A legitimate nested subpath still parses.
        assert!(parse_source("github.com/owner/repo/plugins/dir").is_ok());
    }

    #[test]
    fn rejects_empty_source() {
        assert!(parse_source("").is_err());
        assert!(parse_source("   ").is_err());
    }

    #[test]
    fn pick_highest_tag_respects_a_range() {
        let tags = ["v1.0.0", "v1.2.0", "v2.0.0", "not-a-version"];
        let req = semver::VersionReq::parse("^1.0.0").unwrap();
        assert_eq!(pick_highest_tag(tags, Some(&req)), Some("v1.2.0"));
    }

    #[test]
    fn pick_highest_tag_with_no_range_picks_the_overall_highest() {
        let tags = ["v1.0.0", "2.0.0", "v1.5.0"];
        assert_eq!(pick_highest_tag(tags, None), Some("2.0.0"));
    }

    #[test]
    fn pick_highest_tag_ignores_non_semver_tags() {
        let tags = ["latest", "release-candidate"];
        assert_eq!(pick_highest_tag(tags, None), None);
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn install_dir_nests_host_owner_repo() {
        let source = parse_source("github.com/owner/repo").unwrap();
        let dir = install_dir(Path::new("/x/.grpctestify/plugins"), &source);
        assert_eq!(
            dir,
            Path::new("/x/.grpctestify/plugins/installed/github.com/owner/repo")
        );
    }

    #[test]
    fn lockfile_path_is_sibling_of_plugins_dir() {
        let path = lockfile_path(Path::new("/x/.grpctestify/plugins"));
        assert_eq!(path, Path::new("/x/.grpctestify/plugins.lock.yaml"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn lockfile_roundtrips_through_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.lock.yaml");
        let mut lock = Lockfile::default();
        lock.upsert(LockedPlugin {
            key: "github.com/owner/repo".to_string(),
            host: "github.com".to_string(),
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            subpath: None,
            requested: Some("main".to_string()),
            resolved_version: None,
            resolved_commit: "abc123".to_string(),
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            files: vec![LockedFile {
                path: "is_even.rhai".to_string(),
                sha256: "deadbeef".to_string(),
            }],
        });
        lock.write(&path).unwrap();

        let read_back = Lockfile::read(&path);
        assert_eq!(read_back.plugins.len(), 1);
        assert_eq!(read_back.plugins[0].resolved_commit, "abc123");
    }

    #[test]
    fn lockfile_read_on_missing_file_is_empty() {
        let lock = Lockfile::read(Path::new("/nonexistent/plugins.lock.yaml"));
        assert!(lock.plugins.is_empty());
    }

    #[test]
    fn upsert_replaces_existing_entry_by_key() {
        let mut lock = Lockfile::default();
        let make = |commit: &str| LockedPlugin {
            key: "github.com/owner/repo".to_string(),
            host: "github.com".to_string(),
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            subpath: None,
            requested: None,
            resolved_version: None,
            resolved_commit: commit.to_string(),
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            files: vec![],
        };
        lock.upsert(make("first"));
        lock.upsert(make("second"));
        assert_eq!(lock.plugins.len(), 1);
        assert_eq!(lock.plugins[0].resolved_commit, "second");
    }

    #[test]
    fn remove_reports_whether_an_entry_existed() {
        let mut lock = Lockfile::default();
        lock.upsert(LockedPlugin {
            key: "github.com/owner/repo".to_string(),
            host: "github.com".to_string(),
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            subpath: None,
            requested: None,
            resolved_version: None,
            resolved_commit: "abc".to_string(),
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            files: vec![],
        });
        assert!(lock.remove("github.com/owner/repo"));
        assert!(!lock.remove("github.com/owner/repo"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn read_manifest_returns_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_manifest(dir.path()), Ok(None));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn read_manifest_parses_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE_NAME),
            "description: A test plugin\ngrpctestify: \">=1.8\"\nfiles:\n  - is_even.rhai\n  - nested/util.rhai\n",
        )
        .unwrap();
        let manifest = read_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(manifest.description.as_deref(), Some("A test plugin"));
        assert_eq!(manifest.grpctestify.as_deref(), Some(">=1.8"));
        assert_eq!(
            manifest.files,
            Some(vec![
                "is_even.rhai".to_string(),
                "nested/util.rhai".to_string()
            ])
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn read_manifest_all_fields_optional() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST_FILE_NAME), "{}").unwrap();
        let manifest = read_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(manifest.description, None);
        assert_eq!(manifest.grpctestify, None);
        assert_eq!(manifest.files, None);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn read_manifest_malformed_yaml_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST_FILE_NAME), "not: [valid: yaml").unwrap();
        let err = read_manifest(dir.path()).unwrap_err();
        assert!(err.contains("malformed"), "{err}");
    }

    #[test]
    fn compat_warning_none_when_no_constraint_declared() {
        let manifest = PluginManifest::default();
        assert_eq!(manifest_compat_warning(&manifest, "1.9.0"), None);
    }

    #[test]
    fn compat_warning_none_when_version_satisfies_range() {
        let manifest = PluginManifest {
            grpctestify: Some(">=1.8".to_string()),
            ..Default::default()
        };
        assert_eq!(manifest_compat_warning(&manifest, "1.9.0"), None);
    }

    #[test]
    fn compat_warning_present_when_version_violates_range() {
        let manifest = PluginManifest {
            grpctestify: Some(">=2.0".to_string()),
            ..Default::default()
        };
        let warning = manifest_compat_warning(&manifest, "1.9.0").unwrap();
        assert!(warning.contains(">=2.0"));
        assert!(warning.contains("1.9.0"));
    }

    #[test]
    fn compat_warning_none_when_constraint_is_unparseable() {
        // Informational only — never blocks, even on the plugin author's own mistake.
        let manifest = PluginManifest {
            grpctestify: Some("not-a-range".to_string()),
            ..Default::default()
        };
        assert_eq!(manifest_compat_warning(&manifest, "1.9.0"), None);
    }
}
