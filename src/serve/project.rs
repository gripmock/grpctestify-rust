use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn safe_file_stem(kind: &str, id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        bail!("Invalid {kind}");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default = "default_tls")]
    pub tls: bool,
    #[serde(default = "default_tls_insecure")]
    pub tls_insecure: bool,
    #[serde(default)]
    pub active_env: Option<String>,
    #[serde(default)]
    pub collections: Option<Vec<String>>,
}

fn default_version() -> u32 {
    1
}
fn default_address() -> String {
    "localhost:4770".into()
}
fn default_protocol() -> String {
    "grpc".into()
}
fn default_tls() -> bool {
    false
}
fn default_tls_insecure() -> bool {
    false
}

fn env_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!(".env.{}", name))
}

fn env_local_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!(".env.{}.local", name))
}

fn read_text_file(path: &Path) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(Some(content))
}

fn write_text_file(path: &Path, content: &str) -> Result<()> {
    let content = if content.is_empty() || content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    };
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn delete_text_file(path: &Path) -> Result<()> {
    if path.is_file() {
        fs::remove_file(path).with_context(|| format!("Failed to delete {}", path.display()))?;
    }
    Ok(())
}

pub fn detect_project(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(".grpctestify");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

pub fn load_project_settings(root: &Path) -> Result<ProjectSettings> {
    let path = root.join("settings.json");
    let raw = read_text_file(&path)?.ok_or_else(|| anyhow::anyhow!("settings.json not found"))?;
    let settings: ProjectSettings = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(settings)
}

pub fn save_project_settings(root: &Path, settings: &ProjectSettings) -> Result<()> {
    let path = root.join("settings.json");
    let raw = serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;
    write_text_file(&path, &raw)
}

pub fn list_env_files(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root).context("Failed to read project directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(rest) = name.strip_prefix(".env.")
            && !rest.ends_with(".local")
            && !rest.contains('/')
        {
            names.push(rest.to_string());
        }
    }
    names.sort();
    Ok(names)
}

pub fn read_dotenv(root: &Path, name: &str) -> Result<Option<String>> {
    read_text_file(&env_path(root, name))
}

pub fn write_dotenv(root: &Path, name: &str, content: &str) -> Result<()> {
    write_text_file(&env_path(root, name), content)
}

pub fn read_dotenv_local(root: &Path, name: &str) -> Result<Option<String>> {
    read_text_file(&env_local_path(root, name))
}

pub fn write_dotenv_local(root: &Path, name: &str, content: &str) -> Result<()> {
    ensure_workbench_ignored(root);
    write_text_file(&env_local_path(root, name), content)
}

pub const IGNORED_BY_THE_WORKBENCH: [&str; 4] = ["*.local", "shares/", "history/", "reports/"];

pub fn ensure_workbench_ignored(root: &Path) {
    let marker = root.join(".gitignore");
    let held = fs::read_to_string(&marker).unwrap_or_default();
    let missing: Vec<&str> = IGNORED_BY_THE_WORKBENCH
        .iter()
        .copied()
        .filter(|line| !held.lines().any(|held| held.trim() == *line))
        .collect();
    if missing.is_empty() {
        return;
    }
    let mut next = held;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    for line in missing {
        next.push_str(line);
        next.push('\n');
    }
    let _ = fs::write(&marker, next);
}

pub fn parse_dotenv(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim();
        if key.is_empty() {
            continue;
        }
        let raw = line[eq + 1..].trim();
        let value = match (raw.chars().next(), raw.chars().last()) {
            (Some('"'), Some('"')) | (Some('\''), Some('\'')) if raw.len() >= 2 => {
                raw[1..raw.len() - 1].to_string()
            }
            _ => raw
                .split_once(" #")
                .map_or(raw, |(v, _)| v)
                .trim()
                .to_string(),
        };
        map.insert(key.to_string(), value);
    }
    map
}

pub fn env_variables(root: &Path, name: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut vars = parse_dotenv(&read_dotenv(root, name)?.unwrap_or_default());
    for (k, v) in parse_dotenv(&read_dotenv_local(root, name)?.unwrap_or_default()) {
        vars.insert(k, v);
    }
    Ok(vars)
}

pub fn project_variables(dir: &Path) -> std::collections::HashMap<String, String> {
    let Some(root) = detect_project(dir) else {
        return std::collections::HashMap::new();
    };
    let Some(name) = load_project_settings(&root)
        .ok()
        .and_then(|s| s.active_env)
        .filter(|n| !n.trim().is_empty())
    else {
        return std::collections::HashMap::new();
    };
    env_variables(&root, &name).unwrap_or_default()
}

pub fn address_of(vars: &std::collections::HashMap<String, String>) -> Option<String> {
    ["GRPCTESTIFY_ADDRESS", "GRPC_ADDRESS"]
        .iter()
        .find_map(|key| vars.get(*key))
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
}

pub fn delete_dotenv_local(root: &Path, name: &str) -> Result<()> {
    delete_text_file(&env_local_path(root, name))
}

pub fn delete_dotenv(root: &Path, name: &str) -> Result<()> {
    delete_text_file(&env_path(root, name))?;
    delete_text_file(&env_local_path(root, name))
}

pub fn list_history_sessions(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("history");
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut sessions: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in fs::read_dir(&dir).context("Failed to read history directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".jsonl") {
            let touched = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            sessions.push((touched, id.to_string()));
        }
    }
    sessions.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(sessions.into_iter().map(|(_, id)| id).collect())
}

pub fn read_history_session(root: &Path, session: &str) -> Result<Vec<String>> {
    safe_file_stem("session id", session)?;
    let path = root.join("history").join(format!("{}.jsonl", session));
    if !path.is_file() {
        return Ok(vec![]);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

const HISTORY_MAX_BYTES: u64 = 4 * 1024 * 1024;
const HISTORY_KEEP_LINES: usize = 500;

const SECRET_KEYS: [&str; 6] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
];

pub fn is_secret_header(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SECRET_KEYS.contains(&lower.as_str()) || lower.ends_with("-token")
}

pub fn redact_secrets(
    headers: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| {
            if is_secret_header(k) {
                (k.clone(), "<redacted>".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

pub const KEEP_SESSIONS: usize = 20;

pub fn prune_history_sessions(root: &Path, keep: usize) -> usize {
    let dir = root.join("history");
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    let mut sessions: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if sessions.len() <= keep {
        return 0;
    }
    sessions.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let mut removed = 0;
    for (_, path) in sessions.into_iter().skip(keep) {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub fn trimmed_history(existing: &str, max_bytes: u64, keep: usize) -> Option<String> {
    if existing.len() as u64 <= max_bytes {
        return None;
    }
    let lines: Vec<&str> = existing.lines().collect();
    let start = lines.len().saturating_sub(keep);
    Some(format!("{}\n", lines[start..].join("\n")))
}

pub fn append_history_entry(root: &Path, session: &str, entry: &str) -> Result<()> {
    safe_file_stem("session id", session)?;
    let dir = root.join("history");
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).ok();
    }
    let path = dir.join(format!("{}.jsonl", session));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    writeln!(file, "{}", entry)
        .with_context(|| format!("Failed to append to {}", path.display()))?;

    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > HISTORY_MAX_BYTES
        && let Ok(existing) = std::fs::read_to_string(&path)
        && let Some(kept) = trimmed_history(&existing, HISTORY_MAX_BYTES, HISTORY_KEEP_LINES)
    {
        std::fs::write(&path, kept).ok();
    }
    Ok(())
}

pub fn ensure_shares_dir(shares_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(shares_dir)?;
    let marker = shares_dir.join(".gitignore");
    if !marker.exists() {
        let _ = fs::write(&marker, "# Written by `grpctestify play`; not source.\n*\n");
    }
    Ok(shares_dir.to_path_buf())
}

pub fn write_share(shares_dir: &Path, id: &str, content: &str) -> Result<()> {
    safe_file_stem("share id", id)?;
    let dir = ensure_shares_dir(shares_dir)?;
    fs::write(dir.join(format!("{}.json", id)), content)?;
    Ok(())
}

pub fn read_share(shares_dir: &Path, id: &str) -> Result<Option<String>> {
    safe_file_stem("share id", id)?;
    let path = shares_dir.join(format!("{}.json", id));
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(&path)?))
}

pub fn delete_share(shares_dir: &Path, id: &str) -> Result<()> {
    safe_file_stem("share id", id)?;
    let path = shares_dir.join(format!("{}.json", id));
    if path.is_file() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn cleanup_expired_shares(shares_dir: &Path) -> Result<usize> {
    let now = apif_cfg_runtime::now_unix_millis() as i64;
    let mut removed = 0;
    if !shares_dir.is_dir() {
        return Ok(0);
    }
    for entry in fs::read_dir(shares_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(json) = fs::read_to_string(&path)
            && let Ok(share) = serde_json::from_str::<super::ShareState>(&json)
            && share.expires_at < now
        {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn init_project_dir(root: &Path) -> Result<()> {
    let dot = root.join(".grpctestify");

    fs::create_dir_all(dot.join("collections"))
        .context("Failed to create .grpctestify/collections")?;
    fs::create_dir_all(dot.join("history")).context("Failed to create .grpctestify/history")?;
    fs::create_dir_all(dot.join("shares")).context("Failed to create .grpctestify/shares")?;

    let settings = ProjectSettings {
        version: 1,
        address: "localhost:4770".into(),
        protocol: "grpc".into(),
        tls: false,
        tls_insecure: false,
        active_env: Some("example".into()),
        collections: None,
    };
    save_project_settings(&dot, &settings)?;

    fs::write(
        dot.join(".env.example"),
        r#"# Environment template
# Copy this file to create a new shared environment:
#   cp .env.example .env.staging
#
# Then copy to create your local overrides:
#   cp .env.staging .env.staging.local
#
# GRPC_ADDRESS is a special key that sets the gRPC target
# address for this environment. Leave empty to use the
# global address from settings.json.
GRPC_ADDRESS=

# Add your {{KEY}} variables below.
# Empty values are placeholders for secrets.
# Fill them in .env.{name}.local (gitignored).
# KEY=
"#,
    )?;

    fs::write(
        dot.join(".gitignore"),
        format!("{}\n", IGNORED_BY_THE_WORKBENCH.join("\n")),
    )?;
    fs::write(dot.join(".gitkeep"), "")?;
    fs::write(dot.join("collections/.gitkeep"), "")?;
    fs::write(dot.join("history/.gitkeep"), "")?;
    fs::write(dot.join("shares/.gitkeep"), "")?;

    Ok(())
}

#[cfg(test)]
mod redaction_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn a_credential_is_written_down_by_name_only() {
        let headers: HashMap<String, String> = [
            ("authorization", "Bearer super-secret"),
            ("Cookie", "session=abc"),
            ("x-api-key", "k-123"),
            ("x-refresh-token", "r-1"),
            ("x-request-id", "7f3a"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let safe = redact_secrets(&headers);
        assert_eq!(safe["authorization"], "<redacted>");
        assert_eq!(
            safe["Cookie"], "<redacted>",
            "the check is case-insensitive"
        );
        assert_eq!(safe["x-api-key"], "<redacted>");
        assert_eq!(safe["x-refresh-token"], "<redacted>");
        assert_eq!(
            safe["x-request-id"], "7f3a",
            "a plain header keeps its value"
        );
        assert_eq!(safe.len(), headers.len(), "the keys stay — only values go");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn a_machine_only_value_brings_its_own_ignore() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        assert!(
            !root.join(".gitignore").exists(),
            "nothing there to begin with"
        );

        write_dotenv_local(root, "example", "AUTH_TOKEN=sk-live\n").expect("write");

        let held = std::fs::read_to_string(root.join(".gitignore")).expect("an ignore beside it");
        assert!(held.lines().any(|l| l.trim() == "*.local"), "{held}");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn an_ignore_that_is_there_is_added_to_rather_than_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join(".gitignore"), "reports/\n").expect("write");

        write_dotenv_local(root, "example", "A=1\n").expect("write");

        let held = std::fs::read_to_string(root.join(".gitignore")).expect("read");
        assert!(held.contains("reports/"), "what was there stays: {held}");
        assert!(
            held.contains("*.local"),
            "and the one that matters is added: {held}"
        );

        write_dotenv_local(root, "other", "B=2\n").expect("write");
        let again = std::fs::read_to_string(root.join(".gitignore")).expect("read");
        assert_eq!(again.matches("*.local").count(), 1, "{again}");
    }

    #[test]
    fn history_keeps_its_tail_and_leaves_a_small_file_alone() {
        let small = "one\ntwo\n";
        assert!(trimmed_history(small, 1024, 500).is_none());

        let long: String = (0..1000).map(|i| format!("line{i}\n")).collect();
        let kept = trimmed_history(&long, 100, 3).expect("a long file is trimmed");
        assert_eq!(kept, "line997\nline998\nline999\n");
    }
}

#[cfg(test)]
mod text_file_tests {
    use super::*;

    #[cfg_attr(miri, ignore)]
    #[test]
    fn what_is_written_ends_the_way_a_text_file_ends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let settings = ProjectSettings {
            version: default_version(),
            address: default_address(),
            protocol: default_protocol(),
            tls: default_tls(),
            tls_insecure: default_tls_insecure(),
            active_env: None,
            collections: None,
        };
        save_project_settings(root, &settings).expect("saved");
        let held = fs::read_to_string(root.join("settings.json")).expect("read");
        assert!(held.ends_with('\n'), "{held:?}");
        assert!(!held.ends_with("\n\n"), "one newline, not two: {held:?}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_newline_is_not_added_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("env");
        write_text_file(&path, "A=1\n").expect("written");
        assert_eq!(fs::read_to_string(&path).expect("read"), "A=1\n");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn an_empty_file_stays_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty");
        write_text_file(&path, "").expect("written");
        assert_eq!(fs::read_to_string(&path).expect("read"), "");
    }
}

#[cfg(test)]
mod session_sweep_tests {
    use super::*;

    fn session(dir: &Path, name: &str, age_secs: u64) {
        let path = dir.join(format!("{name}.jsonl"));
        fs::write(&path, "{}\n").expect("write");
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(when)
            .expect("stamp");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn the_oldest_sessions_go_and_the_newest_stay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let history = root.join("history");
        fs::create_dir_all(&history).expect("dir");
        fs::write(history.join(".gitkeep"), "").expect("write");
        for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
            session(&history, name, (i as u64 + 1) * 60);
        }

        assert_eq!(prune_history_sessions(root, 2), 2);
        assert!(history.join("a.jsonl").exists(), "newest kept");
        assert!(history.join("b.jsonl").exists(), "second newest kept");
        assert!(!history.join("c.jsonl").exists());
        assert!(!history.join("d.jsonl").exists());
        assert!(history.join(".gitkeep").exists());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_project_under_the_limit_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let history = root.join("history");
        fs::create_dir_all(&history).expect("dir");
        session(&history, "only", 10);

        assert_eq!(prune_history_sessions(root, KEEP_SESSIONS), 0);
        assert!(history.join("only.jsonl").exists());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn nothing_to_sweep_is_not_a_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(prune_history_sessions(dir.path(), 2), 0);
    }
}

#[cfg(test)]
mod ignore_tests {
    use super::*;

    #[cfg_attr(miri, ignore)]
    #[test]
    fn every_line_the_workbench_needs_is_added_to_an_existing_ignore() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::write(root.join(".gitignore"), "*.local\n").expect("write");

        ensure_workbench_ignored(root);

        let held = fs::read_to_string(root.join(".gitignore")).expect("read");
        for line in IGNORED_BY_THE_WORKBENCH {
            assert!(held.lines().any(|l| l.trim() == line), "{line} in {held}");
        }
        assert_eq!(
            held.matches("*.local").count(),
            1,
            "not written twice: {held}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn what_the_project_already_ignores_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::write(root.join(".gitignore"), "node_modules/\n").expect("write");

        ensure_workbench_ignored(root);

        let held = fs::read_to_string(root.join(".gitignore")).expect("read");
        assert!(held.contains("node_modules/"), "{held}");
        assert!(held.contains("history/"), "{held}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn ensuring_it_again_changes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_workbench_ignored(root);
        let once = fs::read_to_string(root.join(".gitignore")).expect("read");
        ensure_workbench_ignored(root);
        assert_eq!(
            once,
            fs::read_to_string(root.join(".gitignore")).expect("read")
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    #[cfg_attr(miri, ignore)]
    fn making_a_share_sweeps_the_expired_ones() {
        let dir = std::env::temp_dir().join(format!("shares-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");

        let share = |id: &str, expires_at: i64| {
            format!(
                "{{\"id\":\"{id}\",\"endpoint\":\"a.B/C\",\"headers\":{{}},\"bodies\":[],\"created_at\":0,\"expires_at\":{expires_at},\"access_count\":0}}"
            )
        };
        write_share(&dir, "stale", &share("stale", 1)).expect("write");
        let far = apif_cfg_runtime::now_unix_millis() as i64 + 86_400_000;
        write_share(&dir, "fresh", &share("fresh", far)).expect("write");

        let removed = cleanup_expired_shares(&dir).expect("swept");
        let stale_gone = read_share(&dir, "stale").expect("read").is_none();
        let fresh_kept = read_share(&dir, "fresh").expect("read").is_some();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(removed, 1);
        assert!(stale_gone);
        assert!(fresh_kept);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn history_sessions_come_back_newest_first() {
        let dir = std::env::temp_dir().join(format!("history-order-{}", std::process::id()));
        let history = dir.join("history");
        std::fs::create_dir_all(&history).expect("dir");

        std::fs::write(history.join("older.jsonl"), "{}\n").expect("write");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(history.join("newer.jsonl"), "{}\n").expect("write");

        let sessions = list_history_sessions(&dir).expect("listed");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(sessions, vec!["newer".to_string(), "older".to_string()]);
    }
    #[test]
    fn an_environment_names_its_target_under_either_key() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("GRPC_ADDRESS".to_string(), "from-grpc:4770".to_string());
        assert_eq!(address_of(&vars).as_deref(), Some("from-grpc:4770"));

        vars.insert(
            "GRPCTESTIFY_ADDRESS".to_string(),
            "from-grpctestify:4770".to_string(),
        );
        assert_eq!(address_of(&vars).as_deref(), Some("from-grpctestify:4770"));
    }

    #[test]
    fn an_environment_with_no_target_has_none() {
        let mut vars = std::collections::HashMap::new();
        assert!(address_of(&vars).is_none());
        vars.insert("GRPC_ADDRESS".to_string(), "  ".to_string());
        assert!(address_of(&vars).is_none());
    }

    use super::*;

    #[cfg_attr(miri, ignore)]
    #[test]
    fn client_supplied_ids_cannot_escape_their_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        for bad in [
            "../../../../pwned",
            "..",
            "a/b",
            "a\\b",
            "",
            "with space",
            "dot.name",
        ] {
            assert!(
                append_history_entry(root, bad, "{}").is_err(),
                "history id {bad:?} must be rejected"
            );
            assert!(
                write_share(root, bad, "{}").is_err(),
                "share id {bad:?} must be rejected"
            );
            assert!(read_share(root, bad).is_err());
            assert!(delete_share(root, bad).is_err());
            assert!(read_history_session(root, bad).is_err());
        }

        append_history_entry(root, "abc-123_XYZ", "{}").unwrap();
        assert!(root.join("history/abc-123_XYZ.jsonl").is_file());
        assert!(!root.parent().unwrap().join("pwned.jsonl").exists());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn deleting_an_environment_takes_its_local_overrides_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write_dotenv(root, "staging", "A=1\n").unwrap();
        write_dotenv_local(root, "staging", "A=2\n").unwrap();
        write_dotenv(root, "prod", "A=3\n").unwrap();

        delete_dotenv(root, "staging").unwrap();

        assert_eq!(read_dotenv(root, "staging").unwrap(), None);
        assert_eq!(read_dotenv_local(root, "staging").unwrap(), None);
        assert_eq!(list_env_files(root).unwrap(), vec!["prod".to_string()]);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn deleting_an_environment_twice_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        delete_dotenv(dir.path(), "never-was").unwrap();
    }

    #[test]
    fn dotenv_is_read_the_way_every_reader_reads_it() {
        let vars = parse_dotenv(
            "# a comment\n\
             HOST=example.com\n\
             export LEGACY=1\n\
             QUOTED=\"two words\"\n\
             INLINE=value # trailing\n\
             HOST=later-wins\n\
             = novalue\n",
        );

        assert_eq!(vars.get("HOST").map(String::as_str), Some("later-wins"));
        assert_eq!(vars.get("LEGACY").map(String::as_str), Some("1"));
        assert_eq!(vars.get("QUOTED").map(String::as_str), Some("two words"));
        assert_eq!(vars.get("INLINE").map(String::as_str), Some("value"));
        assert_eq!(vars.len(), 4, "a nameless line defines nothing: {vars:?}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_local_value_wins_over_the_shared_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write_dotenv(root, "staging", "TOKEN=placeholder\nHOST=staging:4770\n").unwrap();
        write_dotenv_local(root, "staging", "TOKEN=mine\n").unwrap();

        let vars = env_variables(root, "staging").unwrap();
        assert_eq!(vars.get("TOKEN").map(String::as_str), Some("mine"));
        assert_eq!(vars.get("HOST").map(String::as_str), Some("staging:4770"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_run_starts_with_the_active_environments_variables() {
        let dir = tempfile::tempdir().unwrap();
        init_project_dir(dir.path()).unwrap();
        let root = dir.path().join(".grpctestify");

        write_dotenv(&root, "example", "TOKEN=from-shared\n").unwrap();
        write_dotenv_local(&root, "example", "TOKEN=from-local\n").unwrap();

        let vars = project_variables(dir.path());
        assert_eq!(vars.get("TOKEN").map(String::as_str), Some("from-local"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_directory_that_is_not_a_project_supplies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_variables(dir.path()).is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_fresh_project_verifies_tls_certificates() {
        assert!(!default_tls_insecure());
        let dir = tempfile::tempdir().unwrap();
        init_project_dir(dir.path()).unwrap();
        let settings = load_project_settings(&dir.path().join(".grpctestify")).unwrap();
        assert!(!settings.tls_insecure);
        let bare: ProjectSettings = serde_json::from_str("{}").unwrap();
        assert!(!bare.tls_insecure);
    }
}
