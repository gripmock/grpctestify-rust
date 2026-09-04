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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

pub mod api;
pub mod assets;
pub mod bench_job;
pub mod eval_api;
pub mod jobs;
pub mod lsp_api;
pub mod project;
pub mod reports;

pub struct PlayState {
    pub collections_dir: PathBuf,
    pub collections_dirs: Vec<PathBuf>,
    pub shares_dir: PathBuf,
    pub project_root: Option<PathBuf>,
    pub project_settings: Option<ProjectSettings>,
    pub history_lock: tokio::sync::Mutex<()>,
    pub write_lock: tokio::sync::Mutex<()>,
    pub collections_mtime: Arc<AtomicU64>,
    pub jobs: Arc<jobs::JobRegistry>,
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
    #[serde(default)]
    pub redacted: Vec<String>,
}

async fn static_handler(Path(path): Path<String>) -> Response {
    assets::handle_embedded(&format!("assets/{}", path)).await
}

async fn index_handler() -> Response {
    assets::handle_embedded("").await
}

async fn spa_fallback(Path(path): Path<String>) -> Response {
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
        build: assets::build_id(),
    })
}

#[derive(Serialize, Default)]
pub struct ServerEnv {
    pub address: Option<String>,
    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub tls_server_name: Option<String>,
    pub compression: Option<String>,
}

fn server_env() -> ServerEnv {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    ServerEnv {
        address: var(crate::config::ENV_GRPCTESTIFY_ADDRESS),
        tls_ca: var("GRPCTESTIFY_TLS_CA_FILE"),
        tls_cert: var("GRPCTESTIFY_TLS_CERT_FILE"),
        tls_key: var("GRPCTESTIFY_TLS_KEY_FILE"),
        tls_server_name: var("GRPCTESTIFY_TLS_SERVER_NAME"),
        compression: var("GRPCTESTIFY_COMPRESSION"),
    }
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub version: String,
    pub status: String,
    pub project: Option<api::ProjectInfo>,
    pub env: ServerEnv,
    pub collections_mtime: u64,
    pub workspace: String,
    pub root: String,
    pub shares_path: String,
}

pub async fn info_handler(State(state): State<Arc<PlayState>>) -> Json<InfoResponse> {
    let project = if state.project_root.is_some() {
        Some(api::project_info_inner(&state))
    } else {
        None
    };
    let dir = api::primary_dir(&state);
    Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".into(),
        project,
        env: server_env(),
        collections_mtime: state.collections_mtime.load(Ordering::Relaxed),
        workspace: dir
            .canonicalize()
            .unwrap_or_else(|_| dir.to_path_buf())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string()),
        root: workspace_identity(dir),
        shares_path: if state.project_root.is_some() {
            ".grpctestify/shares".to_string()
        } else {
            "shares".to_string()
        },
    })
}

fn workspace_identity(dir: &std::path::Path) -> String {
    use sha2::Digest;
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let digest = sha2::Sha256::digest(canonical.to_string_lossy().as_bytes());
    let short: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());
    format!("{name}#{short}")
}

pub(super) fn redact_query(query: &str) -> String {
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if key == "token" => format!("{key}=<redacted>"),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

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
        .map(|q| format!("?{}", redact_query(q)))
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

fn host_header_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if host.matches(':').count() > 1 {
        host
    } else {
        host.rsplit_once(':').map_or(host, |(h, _)| h)
    }
}

fn host_is_loopback(host: &str) -> bool {
    let name = host_header_name(host);
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name == "::1"
}

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

pub struct PlayToken {
    pub value: String,
    pub generated: bool,
}

pub fn token_for_bind(host: &str) -> Option<PlayToken> {
    if host_is_loopback(host) {
        return None;
    }
    match std::env::var("GRPCTESTIFY_PLAY_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(value) => Some(PlayToken {
            value,
            generated: false,
        }),
        None => Some(PlayToken {
            value: uuid::Uuid::new_v4().to_string(),
            generated: true,
        }),
    }
}

pub fn needs_token(path: &str) -> bool {
    path.starts_with("/api/")
}

pub(super) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    let longest = a.len().max(b.len());
    for i in 0..longest {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

pub fn request_has_token(
    headers: &axum::http::HeaderMap,
    query: Option<&str>,
    expected: &str,
) -> bool {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    if bearer.is_some_and(|given| ct_eq(given.as_bytes(), expected.as_bytes())) {
        return true;
    }
    query
        .into_iter()
        .flat_map(|q| q.split('&'))
        .filter_map(|pair| pair.split_once('='))
        .any(|(key, value)| key == "token" && ct_eq(value.as_bytes(), expected.as_bytes()))
}

pub fn build_app(state: Arc<PlayState>) -> Router {
    let base_routes = Router::new()
        .route("/", get(index_handler))
        .route("/assets/{*path}", get(static_handler))
        .route("/api/collections", get(api::list_collections))
        .route("/api/collections/{*path}", get(api::get_collection))
        .route("/api/save", post(api::save_collection))
        .route("/api/bench/compare", post(api::bench_compare))
        .route("/api/changed", get(api::changed_collections))
        .route(
            "/api/save-structured",
            post(api::save_collection_structured),
        )
        .route(
            "/api/preview-structured",
            post(api::preview_collection_structured),
        )
        .route("/api/fmt", post(api::format_content))
        .route("/api/complete", post(lsp_api::complete))
        .route("/api/hover", post(lsp_api::hover))
        .route("/api/snippets", get(lsp_api::snippets))
        .route("/api/explain", post(lsp_api::explain))
        .route("/api/eval/assert", post(eval_api::eval_assert))
        .route("/api/eval/query", post(eval_api::eval_query))
        .route("/api/eval/regex", post(eval_api::eval_regex))
        .route("/api/call", post(api::execute_call))
        .route("/api/run", post(api::execute_test))
        .route("/api/jobs", post(jobs::create_job))
        .route("/api/jobs", get(jobs::list_jobs))
        .route("/api/jobs/{id}", get(jobs::get_job))
        .route("/api/jobs/{id}/events", get(jobs::job_events))
        .route("/api/jobs/{id}/report/{name}", get(jobs::job_report))
        .route("/api/jobs/{id}/cancel", post(jobs::cancel_job))
        .route("/api/diagnostics", post(api::get_diagnostics))
        .route("/api/target-health", post(api::target_health))
        .route("/api/check", post(api::check_files))
        .route("/api/versions", post(api::file_versions))
        .route("/api/reflect", post(api::reflect_server))
        .route("/api/import-grpcurl", post(api::import_grpcurl))
        .route("/api/grpcurl", post(api::generate_grpcurl))
        .route("/api/call-command", post(api::generate_call_command))
        .route("/api/schema-fill", post(api::schema_fill))
        .route("/api/scaffold", post(api::scaffold))
        .route("/api/docs", post(api::docs))
        .route("/api/proto-source", post(api::proto_source))
        .route("/api/proto-upload", post(api::proto_upload))
        .route("/api/proto-files", get(api::proto_files))
        .route("/api/data-files", get(api::data_files))
        .route("/api/bench/profiles", get(api::bench_profiles))
        .route("/api/dir/{*path}", post(api::create_directory))
        .route("/api/move", post(api::move_item))
        .route("/api/rename-variable", post(api::rename_variable_endpoint))
        .route("/api/chain", post(api::chain_edit))
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
        .route("/api/variables", get(api::list_variables))
        .route("/api/references/{*path}", get(api::list_references))
        .route("/api/project/env/list", get(api::project_env_list))
        .route("/api/project/env/{name}", get(api::project_env_get))
        .route("/api/project/env/{name}", put(api::project_env_put))
        .route("/api/project/env/{name}", delete(api::project_env_delete))
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

    base_routes
        .merge(project_routes)
        .route("/{*path}", get(spa_fallback))
        .layer(axum::middleware::from_fn(access_log_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn start_play_server(host: &str, port: u16, dir: PathBuf) -> Result<()> {
    apif_plugins::trust::set_non_interactive();
    let project_root = project::detect_project(&dir);
    if let Some(root) = project_root.as_ref() {
        project::ensure_workbench_ignored(root);
    }

    let collections_dir = project_root
        .as_ref()
        .map(|r| r.join("collections"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| dir.clone());

    let collections_dirs = if let Some(ref root) = project_root {
        let mut dirs = vec![collections_dir.clone()];
        if let Ok(settings) = project::load_project_settings(root)
            && let Some(ref extra) = settings.collections
        {
            let root_canon = root.canonicalize().unwrap_or_else(|_| root.clone());
            for p in extra {
                let resolved = root.join(p);
                if !resolved.is_dir() {
                    continue;
                }
                match resolved.canonicalize() {
                    Ok(canon) if canon.starts_with(&root_canon) => dirs.push(resolved),
                    _ => tracing::warn!(
                        "ignoring collections entry outside the project root: {}",
                        resolved.display()
                    ),
                }
            }
        }
        dirs
    } else {
        vec![collections_dir.clone()]
    };

    let collections_dir_display = collections_dir.display().to_string();

    let collections_mtime = Arc::new(AtomicU64::new(0));

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
        write_lock: tokio::sync::Mutex::new(()),
        collections_mtime,
        jobs: Default::default(),
    });

    let sweep_root = project_root.clone();
    tokio::task::spawn_blocking(move || {
        let _ = project::cleanup_expired_shares(&shares_dir);
        if let Some(root) = sweep_root {
            project::prune_history_sessions(&root, project::KEEP_SESSIONS);
        }
    });

    let app = build_app(state);

    let bound_loopback = host_is_loopback(host);
    let app = if bound_loopback {
        app.layer(axum::middleware::from_fn(loopback_host_guard))
    } else {
        app
    };

    let token = token_for_bind(host);
    let app = match &token {
        Some(play_token) => {
            let expected = play_token.value.clone();
            app.layer(axum::middleware::from_fn(
                move |req: axum::http::Request<Body>, next: axum::middleware::Next| {
                    let expected = expected.clone();
                    async move {
                        if !needs_token(req.uri().path())
                            || request_has_token(req.headers(), req.uri().query(), &expected)
                        {
                            next.run(req).await
                        } else {
                            (
                                StatusCode::UNAUTHORIZED,
                                "This workbench is bound to a network address and needs its token — start it again to read the one it prints, or set GRPCTESTIFY_PLAY_TOKEN.",
                            )
                                .into_response()
                        }
                    }
                },
            ))
        }
        None => app,
    };

    let addr = if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    };
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!(
        "🎨 {bold}grpctestify play{reset} v{version}",
        bold = ansi!(ANSI_BOLD),
        version = version,
        reset = ansi!(ANSI_RESET),
    );
    match &token {
        Some(play_token) => {
            println!(
                "   {dim}➜{reset}  http://{host}:{port}/?token={token}",
                dim = ansi!(ANSI_BOLD),
                reset = ansi!(ANSI_RESET),
                host = host_header_name(host),
                port = port,
                token = play_token.value,
            );
            println!(
                "   {dim}bound to {host} — every request needs this token{reset}",
                dim = ansi!(ANSI_BOLD),
                reset = ansi!(ANSI_RESET),
                host = host,
            );
            if play_token.generated {
                println!(
                    "   {dim}set GRPCTESTIFY_PLAY_TOKEN to keep one across restarts{reset}",
                    dim = ansi!(ANSI_BOLD),
                    reset = ansi!(ANSI_RESET),
                );
            }
        }
        None => println!(
            "   {dim}➜{reset}  http://localhost:{port}",
            dim = ansi!(ANSI_BOLD),
            reset = ansi!(ANSI_RESET),
            port = port
        ),
    }
    if let Some(ref root) = project_root {
        println!("   project  {root}", root = root.display());
        if let Ok(envs) = project::list_env_files(root)
            && !envs.is_empty()
        {
            println!("   envs     {envs}", envs = envs.join(", "));
        }
    }
    println!("   dirs     {dir}", dir = collections_dir_display);
    println!(
        "   {dim}this release names the workspace differently, so tabs saved by an older \
         session are let go once{reset}",
        dim = ansi!(ANSI_BOLD),
        reset = ansi!(ANSI_RESET),
    );
    println!();

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
    fn a_network_bind_asks_for_a_token_and_loopback_does_not() {
        assert!(token_for_bind("127.0.0.1").is_none());
        assert!(token_for_bind("localhost").is_none());
        assert!(token_for_bind("[::1]").is_none());

        let made = token_for_bind("0.0.0.0").expect("a network bind needs one");
        assert!(made.generated);
        assert!(made.value.len() >= 32, "{}", made.value);

        unsafe { std::env::set_var("GRPCTESTIFY_PLAY_TOKEN", "  chosen-by-the-operator  ") };
        let given = token_for_bind("0.0.0.0").expect("a network bind needs one");
        unsafe { std::env::remove_var("GRPCTESTIFY_PLAY_TOKEN") };
        assert!(!given.generated);
        assert_eq!(given.value, "chosen-by-the-operator");
    }

    #[test]
    fn the_token_guards_the_data_and_not_the_page_that_reads_it() {
        assert!(needs_token("/api/collections"));
        assert!(needs_token("/api/jobs/1/events"));
        assert!(!needs_token("/"));
        assert!(!needs_token("/assets/index-abc.js"));
        assert!(!needs_token("/c/auth/login.gctf"));
    }

    #[test]
    fn a_request_carries_its_token_in_a_header_or_in_the_url() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!request_has_token(&headers, None, "secret"));
        assert!(!request_has_token(&headers, Some("token=other"), "secret"));

        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer secret"),
        );
        assert!(request_has_token(&headers, None, "secret"));

        let empty = axum::http::HeaderMap::new();
        assert!(request_has_token(&empty, Some("token=secret"), "secret"));
        assert!(request_has_token(
            &empty,
            Some("since=1&token=secret"),
            "secret"
        ));
        assert!(!request_has_token(&empty, Some("token=secre"), "secret"));
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
        assert!(!host_is_loopback("127.0.0.1.evil.example"));
        assert!(!host_is_loopback(""));
    }

    #[test]
    fn the_access_log_never_prints_a_token() {
        assert_eq!(redact_query("token=abc123"), "token=<redacted>");
        assert_eq!(
            redact_query("since=1&token=abc123&x=y"),
            "since=1&token=<redacted>&x=y"
        );
        assert_eq!(redact_query("since=1"), "since=1");
        assert_eq!(redact_query("tokens=abc"), "tokens=abc");
    }

    #[test]
    fn tokens_compare_without_leaking_where_they_differ() {
        assert!(ct_eq(b"secret", b"secret"));
        assert!(!ct_eq(b"secret", b"secreT"));
        assert!(!ct_eq(b"secret", b"secre"));
        assert!(!ct_eq(b"", b"a"));
        assert!(!ct_eq(b"", &[0u8; 256]));
        assert!(ct_eq(b"", b""));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn the_workspace_identity_names_the_folder_and_not_where_it_lives() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inner = dir.path().join("app");
        std::fs::create_dir_all(&inner).expect("mkdir");
        let identity = workspace_identity(&inner);
        assert!(identity.starts_with("app#"), "{identity}");
        assert_eq!(identity.len(), "app#".len() + 8, "{identity}");
        assert!(
            !identity.contains(&*dir.path().to_string_lossy()),
            "{identity}"
        );
        assert_eq!(identity, workspace_identity(&inner));

        let other = tempfile::tempdir().expect("tempdir");
        let same_name = other.path().join("app");
        std::fs::create_dir_all(&same_name).expect("mkdir");
        assert_ne!(identity, workspace_identity(&same_name));
    }
}
