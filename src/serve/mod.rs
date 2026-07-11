use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::Method,
    response::Response,
    routing::{delete, get, post, put},
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
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
    pub project_root: Option<PathBuf>,
    pub project_settings: Option<ProjectSettings>,
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
    if !path.is_empty()
        && !path.starts_with("api/")
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

pub async fn start_play_server(port: u16, dir: PathBuf) -> Result<()> {
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

    let state = Arc::new(PlayState {
        collections_dir,
        collections_dirs,
        project_root: project_root.clone(),
        project_settings: project_root
            .as_ref()
            .and_then(|r| project::load_project_settings(r).ok()),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

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
        .route("/api/version", get(version_handler))
        .route("/api/health", get(health_handler))
        .route("/api/info", get(info_handler));

    // Always mount project routes (they return `active: false` when no .grpctestify/)
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

    let app = base_routes
        .merge(project_routes)
        .route("/{*path}", get(spa_fallback))
        .layer(axum::middleware::from_fn(access_log_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
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
