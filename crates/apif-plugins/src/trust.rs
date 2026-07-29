//! Trust gate for `.rhai` scripts.
//!
//! Rhai runs a script's top-level statements, not just the hook bodies it
//! defines, so cloning someone else's repository used to be enough to execute
//! their code. Approval is per script *content*: an edit is a new decision.
//!
//! The store lives under the user's home, never in the repository — a trust
//! file a repository could ship would defeat the point. It is the one thing
//! this crate creates in `~/.grpctestify`, and only on an explicit `y`.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Opt-in for CI, where the checkout is already trusted.
const TRUST_ALL_ENV: &str = "GRPCTESTIFY_TRUST_PLUGINS";
/// Kill switch, wins over everything else.
const NO_PLUGINS_ENV: &str = "GRPCTESTIFY_NO_PLUGINS";

/// From the home directory, not from `plugins/` — a project-only user has no
/// `~/.grpctestify/plugins` and still needs their approvals remembered.
fn store_path() -> Option<PathBuf> {
    Some(store_path_in(&crate::rhai_plugin::user_state_dir()?))
}

fn store_path_in(state_dir: &Path) -> PathBuf {
    state_dir.join("trusted_plugins.json")
}

fn read_store_at(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_store_at(path: &Path, store: &BTreeMap<String, String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(path, raw);
    }
}

fn read_store() -> BTreeMap<String, String> {
    store_path().map(|p| read_store_at(&p)).unwrap_or_default()
}

fn write_store(store: &BTreeMap<String, String>) {
    if let Some(path) = store_path() {
        write_store_at(&path, store);
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0" && v != "false")
}

fn key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// Terminal only — a non-interactive run must never block, so it denies.
fn confirm(path: &Path, digest: &str) -> bool {
    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        tracing::warn!(
            "refusing to execute untrusted plugin script {} (sha256 {}); \
             run it once interactively to trust it, or set {TRUST_ALL_ENV}=1",
            path.display(),
            &digest[..16]
        );
        return false;
    }

    eprintln!();
    eprintln!("grpctestify wants to execute a script plugin:");
    eprintln!("  {}", path.display());
    eprintln!("  sha256 {}", digest);
    eprintln!("It runs with your privileges. Only allow scripts you have read.");
    if store_path().is_none() {
        // Otherwise the prompt silently reappears every run.
        eprintln!("(No home directory resolved, so this answer cannot be remembered.)");
    }
    eprint!("Execute it? [y/N] ");
    use std::io::Write;
    let _ = std::io::stderr().flush();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim(), "y" | "Y" | "yes" | "YES")
}

/// The part of the decision that needs no prompt. `None` means "ask".
fn settled(
    no_plugins: bool,
    trust_all: bool,
    known: Option<&String>,
    digest: &str,
) -> Option<bool> {
    if no_plugins {
        return Some(false);
    }
    if trust_all {
        return Some(true);
    }
    if known.is_some_and(|k| k == digest) {
        return Some(true);
    }
    None
}

/// `digest` must hash the bytes that were compiled, not a fresh read —
/// otherwise the user approves contents that are not the ones about to run.
pub fn is_trusted(path: &Path, digest: &str) -> bool {
    let entry = key(path);
    let mut store = read_store();

    if let Some(decided) = settled(
        env_flag(NO_PLUGINS_ENV),
        env_flag(TRUST_ALL_ENV),
        store.get(&entry),
        digest,
    ) {
        return decided;
    }
    if !confirm(path, digest) {
        return false;
    }
    store.insert(entry, digest.to_string());
    write_store(&store);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_wins_over_every_other_signal() {
        assert_eq!(settled(true, true, Some(&"d".into()), "d"), Some(false));
    }

    #[test]
    fn opt_in_env_trusts_without_asking() {
        assert_eq!(settled(false, true, None, "d"), Some(true));
    }

    #[test]
    fn a_matching_recorded_hash_is_silent_but_an_edit_re_asks() {
        assert_eq!(settled(false, false, Some(&"d".into()), "d"), Some(true));
        assert_eq!(settled(false, false, Some(&"old".into()), "d"), None);
        assert_eq!(settled(false, false, None, "d"), None);
    }

    // Not driven through `GRPCTESTIFY_HOME`: `cargo test` runs these on threads
    // of one process, so mutating a var production code reads is a data race.
    #[test]
    #[cfg(not(miri))]
    fn an_approval_creates_the_state_dir_and_round_trips() {
        let home = tempfile::tempdir().unwrap();
        let state_dir = home.path().join(".grpctestify");
        let path = store_path_in(&state_dir);

        assert!(!state_dir.exists(), "nothing exists before an approval");
        assert_eq!(read_store_at(&path), BTreeMap::new());

        write_store_at(&path, &BTreeMap::from([("k".into(), "d".into())]));
        assert!(path.is_file());
        assert_eq!(read_store_at(&path).get("k").map(String::as_str), Some("d"));
    }

    // A hand-mangled store must not wedge the gate; it reads as empty.
    #[test]
    #[cfg(not(miri))]
    fn a_corrupt_store_reads_as_empty_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let path = store_path_in(dir.path());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(read_store_at(&path), BTreeMap::new());
    }
}
