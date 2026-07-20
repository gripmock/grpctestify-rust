use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tower_http::trace::TraceLayer;

use crate::serve::project::ProjectSettings;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

pub mod api;
pub mod assets;
pub mod project;

pub struct PlayState {
    /// Primary collections dir (first in `collections_dirs`), backward compat
    pub collections_dir: PathBuf,
    /// All collections directories (primary + extras from settings.json)
    pub collections_dirs: Vec<PathBuf>,
    pub shares_dir: PathBuf,
    pub project_root: Option<PathBuf>,
    pub project_settings: Option<ProjectSettings>,
    /// Serialize history writes to prevent file-level races.
    pub history_lock: tokio::sync::Mutex<()>,
    /// Monotonic timestamp bumped on every collection/env change.
    /// Frontend polls /api/info and compares this value for auto-reload.
    pub collections_mtime: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareState {
    pub id: String,
    pub endpoint: String,
    pub headers: std::collections::HashMap<String, String>,
    pub bodies: Vec<String>,
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub tls_insecure: Option<bool>,
    pub created_at: i64,
    pub expires_at: i64,
    pub access_count: u64,
}

async fn static_handler(Path(path): Path<String>) -> Response {
    assets::handle_embedded(&format!("assets/{}", path)).await
}

async fn index_handler() -> Response {
    assets::handle_embedded("").await
}

/// Serve root-level static files (favicon.ico, manifest.json, etc.)
/// or fall back to index.html for SPA client-side routing.
async fn spa_fallback(Path(path): Path<String>) -> Response {
    // Unknown API routes must be a real 404, not index.html.
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    if !path.is_empty()
        && let Some(resp) = assets::try_get_asset(&path)
    {
        return resp;
    }
    assets::handle_embedded("").await
}

#[derive(Serialize)]
pub struct VersionResponse {
    pub version: String,
}

pub async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub version: String,
    pub status: String,
    pub project: Option<api::ProjectInfo>,
    /// Monotonic counter that increments on every collection/env change.
    /// Frontend uses this for auto-reload without polling the full list.
    pub collections_mtime: u64,
}

/// GET /api/info — unified startup info (version + health + project)
pub async fn info_handler(State(state): State<Arc<PlayState>>) -> Json<InfoResponse> {
    let project = if state.project_root.is_some() {
        Some(api::project_info_inner(&state))
    } else {
        None
    };
    Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".into(),
        project,
        collections_mtime: state.collections_mtime.load(Ordering::Relaxed),
    })
}

// ANSI color codes for terminal output
/* ── ANSI colors (respects NO_COLOR) ────────────── */

fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

macro_rules! ansi {
    ($code:expr) => {{ if use_color() { $code } else { "" } }};
}

const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_RESET: &str = "\x1b[0m";

fn status_color(code: u16) -> &'static str {
    if !use_color() {
        return "";
    }
    if code < 300 {
        ANSI_GREEN
    } else if code < 400 {
        ANSI_CYAN
    } else if code < 500 {
        ANSI_YELLOW
    } else {
        ANSI_RED
    }
}

fn fmt_size(bytes: &str) -> String {
    if let Ok(b) = bytes.parse::<f64>() {
        if b >= 1_000_000.0 {
            format!("{:.1}MB", b / 1_000_000.0)
        } else if b >= 1_000.0 {
            format!("{:.1}KB", b / 1_000.0)
        } else {
            format!("{}B", b as u64)
        }
    } else {
        bytes.to_string()
    }
}

/* ── Access log ──────────────────────────────────── */

async fn access_log_middleware(
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    if path == "/api/health" {
        return next.run(req).await;
    }

    let method = req.method().clone();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let start = Instant::now();
    let response = next.run(req).await;

    let status = response.status().as_u16();
    let duration_s = start.elapsed().as_secs_f64();
    let duration = if duration_s >= 1.0 {
        format!("{:.2}s", duration_s)
    } else {
        format!("{:.1}ms", duration_s * 1000.0)
    };
    let size = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .map(fmt_size)
        .unwrap_or_else(|| "-".into());

    let now = chrono::Local::now().format("%H:%M:%S%.3f");
    let full_path = format!("{}{}", path, query);

    if path.starts_with("/api/") {
        let status_fmt = format!(
            "{bold}{color}{status}{reset}",
            bold = ansi!(ANSI_BOLD),
            color = status_color(status),
            status = status,
            reset = ansi!(ANSI_RESET)
        );
        println!(
            "{} {} {:>7} {:>7} {} {}",
            now, status_fmt, duration, size, method, full_path
        );
    } else if status >= 400 {
        println!("{} {} {} {}", now, status, method, full_path);
    }

    response
}

/// Background file watcher. Runs until the notify channel disconnects
/// (i.e. the watcher is dropped / the process exits).
fn start_file_watcher(
    mtime: Arc<AtomicU64>,
    dirs: &[PathBuf],
    project_root: Option<&std::path::Path>,
) {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let watch_paths: Vec<PathBuf> = {
        let mut p = dirs.to_vec();
        if let Some(r) = project_root {
            p.push(r.to_path_buf());
        }
        p
    };

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = match Watcher::new(tx, Config::default()) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("Failed to start file watcher: {}.", e);
            return;
        }
    };
    for path in &watch_paths {
        if path.is_dir()
            && let Err(e) = watcher.watch(path, RecursiveMode::Recursive)
        {
            tracing::warn!("Cannot watch {}: {}.", path.display(), e);
        }
    }

    let static_paths = watch_paths;

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                // Always bump: the frontend polls /api/info and tolerates
                // spurious increments; a time-based debounce here dropped the
                // trailing event of a burst, missing the final change.
                mtime.fetch_add(1, Ordering::Relaxed);
                if matches!(event.kind, notify::EventKind::Remove(_)) {
                    for w in &static_paths {
                        if w.is_dir() {
                            let _ = watcher.watch(w, RecursiveMode::Recursive);
                        }
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(mpsc::RecvError) => {
                tracing::debug!("File watcher disconnected.");
                return;
            }
        }
    }
}

/// Extract the hostname from a Host header value, stripping any port
/// (`localhost:4755`, `[::1]:4755`, bare IPv6 literals).
fn host_header_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        // Bracketed IPv6: [::1] or [::1]:4755
        rest.split(']').next().unwrap_or("")
    } else if host.matches(':').count() > 1 {
        // Bare IPv6 literal without port
        host
    } else {
        host.rsplit_once(':').map_or(host, |(h, _)| h)
    }
}

/// Is this Host header a loopback name?
fn host_is_loopback(host: &str) -> bool {
    let name = host_header_name(host);
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name == "::1"
}

/// DNS-rebinding guard: even when bound to 127.0.0.1, a malicious site can
/// reach us by pointing its own DNS name at 127.0.0.1 — the browser then
/// sends `Host: attacker.example`. The playground itself is always opened via
/// a loopback name, so rejecting any other Host closes the hole.
async fn loopback_host_guard(
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let host_ok = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_is_loopback)
        .unwrap_or(false);
    if !host_ok {
        return (StatusCode::FORBIDDEN, "Invalid Host header").into_response();
    }
    next.run(req).await
}

/// Build the axum Router from a PlayState. Tests should use this instead of
/// duplicating route registrations.
pub fn build_app(state: Arc<PlayState>) -> Router {
    let base_routes = Router::new()
        .route("/", get(index_handler))
        .route("/assets/{*path}", get(static_handler))
        .route("/api/collections", get(api::list_collections))
        .route("/api/collections/{*path}", get(api::get_collection))
        .route("/api/save", post(api::save_collection))
        .route(
            "/api/save-structured",
            post(api::save_collection_structured),
        )
        .route("/api/call", post(api::execute_call))
        .route("/api/reflect", post(api::reflect_server))
        .route("/api/import-grpcurl", post(api::import_grpcurl))
        .route("/api/grpcurl", post(api::generate_grpcurl))
        .route("/api/schema-fill", post(api::schema_fill))
        .route("/api/proto-upload", post(api::proto_upload))
        .route("/api/proto-files", get(api::proto_files))
        .route("/api/dir/{*path}", post(api::create_directory))
        .route("/api/move", post(api::move_item))
        .route("/api/collections/{*path}", delete(api::delete_collection))
        .route("/api/share", post(api::create_share))
        .route("/api/share/{id}", get(api::get_share))
        .route("/api/version", get(version_handler))
        .route("/api/health", get(health_handler))
        .route("/api/info", get(info_handler));

    let project_routes = Router::new()
        .route("/api/project/info", get(api::project_info))
        .route("/api/project/settings", get(api::project_get_settings))
        .route("/api/project/settings", put(api::project_put_settings))
        .route("/api/project/env/list", get(api::project_env_list))
        .route("/api/project/env/{name}", get(api::project_env_get))
        .route("/api/project/env/{name}", put(api::project_env_put))
        .route(
            "/api/project/env/{name}/merged",
            get(api::project_env_merged),
        )
        .route(
            "/api/project/env/{name}/local",
            get(api::project_env_local_get),
        )
        .route(
            "/api/project/env/{name}/local",
            put(api::project_env_local_put),
        )
        .route(
            "/api/project/env/{name}/local",
            delete(api::project_env_local_delete),
        )
        .route("/api/project/history", get(api::project_history_get));

    // Note: no CORS layer on purpose — the web UI is served same-origin and
    // only fetches relative /api paths; permissive CORS would let any website
    // in the user's browser drive this server.
    base_routes
        .merge(project_routes)
        .route("/{*path}", get(spa_fallback))
        .layer(axum::middleware::from_fn(access_log_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn start_play_server(host: &str, port: u16, dir: PathBuf) -> Result<()> {
    let project_root = project::detect_project(&dir);

    let collections_dir = project_root
        .as_ref()
        .map(|r| r.join("collections"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| dir.clone());

    // Resolve extra collections dirs from project settings
    let collections_dirs = if let Some(ref root) = project_root {
        let mut dirs = vec![collections_dir.clone()];
        if let Ok(settings) = project::load_project_settings(root)
            && let Some(ref extra) = settings.collections
        {
            for p in extra {
                let resolved = root.join(p);
                if resolved.is_dir() {
                    dirs.push(resolved);
                }
            }
        }
        dirs
    } else {
        vec![collections_dir.clone()]
    };

    let collections_dir_display = collections_dir.display().to_string();

    let collections_mtime = Arc::new(AtomicU64::new(0));

    // Start file watcher (runs for the lifetime of the process)
    let w_mtime = collections_mtime.clone();
    let w_dirs: Vec<PathBuf> = collections_dirs.clone();
    let w_root = project_root.clone();
    tokio::task::spawn_blocking(move || {
        start_file_watcher(w_mtime, &w_dirs, w_root.as_deref());
    });

    let shares_dir = project_root
        .as_ref()
        .map(|r| r.join("shares"))
        .unwrap_or_else(|| dir.join("shares"));

    let state = Arc::new(PlayState {
        collections_dir,
        collections_dirs,
        shares_dir: shares_dir.clone(),
        project_root: project_root.clone(),
        project_settings: project_root
            .as_ref()
            .and_then(|r| project::load_project_settings(r).ok()),
        history_lock: tokio::sync::Mutex::new(()),
        collections_mtime,
    });

    // Cleanup expired shares on startup
    tokio::task::spawn_blocking(move || {
        let _ = project::cleanup_expired_shares(&shares_dir);
    });

    let app = build_app(state);

    // When bound to loopback (the default), reject requests whose Host header
    // is not a loopback name — see loopback_host_guard for the rationale.
    // Users who opt into network exposure via --host skip the guard.
    let bound_loopback = host_is_loopback(host);
    let app = if bound_loopback {
        app.layer(axum::middleware::from_fn(loopback_host_guard))
    } else {
        app
    };

    // Bracket bare IPv6 literals for SocketAddr syntax
    let addr = if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    };
    let version = env!("CARGO_PKG_VERSION");
    println!(
        "{bold}grpctestify play v{version}{reset} — http://localhost:{port}",
        bold = ansi!(ANSI_BOLD),
        version = version,
        reset = ansi!(ANSI_RESET),
        port = port
    );
    if let Some(ref root) = project_root {
        println!("  project  {root}", root = root.display());
        if let Ok(envs) = project::list_env_files(root)
            && !envs.is_empty()
        {
            println!("  envs     {envs}", envs = envs.join(", "));
        }
    }
    println!("  dirs     {dir}", dir = collections_dir_display);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_header_name() {
        assert_eq!(host_header_name("localhost"), "localhost");
        assert_eq!(host_header_name("localhost:4755"), "localhost");
        assert_eq!(host_header_name("127.0.0.1:4755"), "127.0.0.1");
        assert_eq!(host_header_name("[::1]:4755"), "::1");
        assert_eq!(host_header_name("[::1]"), "::1");
        assert_eq!(host_header_name("::1"), "::1");
        assert_eq!(host_header_name("evil.example:4755"), "evil.example");
    }

    #[test]
    fn test_host_is_loopback() {
        assert!(host_is_loopback("localhost"));
        assert!(host_is_loopback("LOCALHOST:4755"));
        assert!(host_is_loopback("127.0.0.1:4755"));
        assert!(host_is_loopback("[::1]:4755"));
        assert!(host_is_loopback("::1"));
        assert!(!host_is_loopback("evil.example"));
        assert!(!host_is_loopback("evil.example:4755"));
        assert!(!host_is_loopback("192.168.1.10:4755"));
        // 127.0.0.1 lookalikes must not pass
        assert!(!host_is_loopback("127.0.0.1.evil.example"));
        assert!(!host_is_loopback(""));
    }
}
