use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const TRUST_ALL_ENV: &str = "GRPCTESTIFY_TRUST_PLUGINS";
const NO_PLUGINS_ENV: &str = "GRPCTESTIFY_NO_PLUGINS";

static NON_INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_non_interactive() {
    NON_INTERACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_non_interactive() -> bool {
    NON_INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn may_prompt(non_interactive: bool, stdin_tty: bool, stderr_tty: bool) -> bool {
    !non_interactive && stdin_tty && stderr_tty
}

pub fn untrusted_message(name: &str, path: &Path) -> String {
    format!(
        "@{name}: refusing to execute untrusted plugin script {} — run a test that uses it with `grpctestify run` once in a terminal to approve it, or set {TRUST_ALL_ENV}=1",
        path.display()
    )
}

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

fn confirm(path: &Path, digest: &str) -> bool {
    if !may_prompt(
        is_non_interactive(),
        std::io::stdin().is_terminal(),
        std::io::stderr().is_terminal(),
    ) {
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

    #[cfg_attr(miri, ignore)]
    #[test]
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

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_corrupt_store_reads_as_empty_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let path = store_path_in(dir.path());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(read_store_at(&path), BTreeMap::new());
    }

    #[test]
    fn a_server_never_prompts_even_on_a_terminal() {
        assert!(may_prompt(false, true, true));
        assert!(!may_prompt(false, false, true));
        assert!(!may_prompt(false, true, false));
        assert!(!may_prompt(true, true, true));
    }

    #[test]
    fn the_refusal_says_how_to_approve_the_script() {
        let said = untrusted_message("len", Path::new("/plugins/len.rhai"));
        assert!(
            said.starts_with("@len: refusing to execute untrusted plugin script /plugins/len.rhai"),
            "{said}"
        );
        assert!(
            said.contains("`grpctestify run`"),
            "the approval path has to be a command that actually executes the plugin: {said}"
        );
        assert!(!said.contains("grpctestify check"), "{said}");
        assert!(said.contains(TRUST_ALL_ENV), "{said}");
    }

    #[test]
    fn the_flag_is_process_wide_and_sticks() {
        set_non_interactive();
        assert!(is_non_interactive());
        assert!(!may_prompt(is_non_interactive(), true, true));
    }
}
