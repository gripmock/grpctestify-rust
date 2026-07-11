use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Project-wide settings stored in settings.json
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
    /// Extra collections directories (relative to this project root)
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
    true
}

/* ── private file helpers ───────────────────────────── */

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
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn delete_text_file(path: &Path) -> Result<()> {
    if path.is_file() {
        fs::remove_file(path).with_context(|| format!("Failed to delete {}", path.display()))?;
    }
    Ok(())
}

/* ── public API ─────────────────────────────────────── */

/// Detect whether a `.grpctestify` project directory exists.
pub fn detect_project(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(".grpctestify");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

/// Load and parse settings.json from a project root.
pub fn load_project_settings(root: &Path) -> Result<ProjectSettings> {
    let path = root.join("settings.json");
    let raw = read_text_file(&path)?.ok_or_else(|| anyhow::anyhow!("settings.json not found"))?;
    let settings: ProjectSettings = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(settings)
}

/// Save ProjectSettings to settings.json.
pub fn save_project_settings(root: &Path, settings: &ProjectSettings) -> Result<()> {
    let path = root.join("settings.json");
    let raw = serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;
    write_text_file(&path, &raw)
}

/// List environment names from .env.* files (excluding .local).
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

/// Check whether a .local file exists for the given env.
pub fn env_local_exists(root: &Path, name: &str) -> bool {
    env_local_path(root, name).is_file()
}

/// Read .env.{name} file content.
pub fn read_dotenv(root: &Path, name: &str) -> Result<Option<String>> {
    read_text_file(&env_path(root, name))
}

/// Write .env.{name} file content.
pub fn write_dotenv(root: &Path, name: &str, content: &str) -> Result<()> {
    write_text_file(&env_path(root, name), content)
}

/// Read .env.{name}.local file content.
pub fn read_dotenv_local(root: &Path, name: &str) -> Result<Option<String>> {
    read_text_file(&env_local_path(root, name))
}

/// Write .env.{name}.local file content.
pub fn write_dotenv_local(root: &Path, name: &str, content: &str) -> Result<()> {
    write_text_file(&env_local_path(root, name), content)
}

/// Delete .env.{name}.local file.
pub fn delete_dotenv_local(root: &Path, name: &str) -> Result<()> {
    delete_text_file(&env_local_path(root, name))
}

/// List session IDs from history/*.json files.
pub fn list_history_sessions(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("history");
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&dir).context("Failed to read history directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".json") {
            sessions.push(id.to_string());
        }
    }
    sessions.sort();
    Ok(sessions)
}

/// Read a single session history file.
pub fn read_history_session(root: &Path, session: &str) -> Result<Option<String>> {
    read_text_file(&root.join("history").join(format!("{}.json", session)))
}

/// Write a single session history file.
pub fn write_history_session(root: &Path, session: &str, content: &str) -> Result<()> {
    let dir = root.join("history");
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).ok();
    }
    write_text_file(&dir.join(format!("{}.json", session)), content)
}

/// Create the .grpctestify project directory structure.
pub fn init_project_dir(root: &Path) -> Result<()> {
    let dot = root.join(".grpctestify");

    fs::create_dir_all(dot.join("collections"))
        .context("Failed to create .grpctestify/collections")?;
    fs::create_dir_all(dot.join("history")).context("Failed to create .grpctestify/history")?;

    let settings = ProjectSettings {
        version: 1,
        address: "localhost:4770".into(),
        protocol: "grpc".into(),
        tls: false,
        tls_insecure: true,
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

    fs::write(dot.join(".gitignore"), "*.local\n")?;
    fs::write(dot.join(".gitkeep"), "")?;
    fs::write(dot.join("collections/.gitkeep"), "")?;
    fs::write(dot.join("history/.gitkeep"), "")?;

    Ok(())
}
