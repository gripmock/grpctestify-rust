use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use axum::Router;

use grpctestify::serve::project;
use grpctestify::serve::{self, PlayState};

/// Create a test app with the given collections dir.
/// Delegates to `serve::build_app()` so it always matches production routes.
fn test_app(collections_dir: PathBuf) -> Router {
    let shares_dir = collections_dir.join("../../shares");
    let state = Arc::new(PlayState {
        collections_dir: collections_dir.clone(),
        collections_dirs: vec![collections_dir],
        shares_dir,
        project_root: None,
        project_settings: None,
        history_lock: tokio::sync::Mutex::new(()),
        collections_mtime: Arc::new(AtomicU64::new(0)),
    });
    serve::build_app(state)
}

/// Create a test app with project mode (with .grpctestify directory).
/// Delegates to `serve::build_app()` so it always matches production routes.
fn test_app_project(dir: PathBuf) -> Router {
    let project_root = dir.join(".grpctestify");
    let collections_dir = project_root.join("collections");

    let state = Arc::new(PlayState {
        collections_dir: collections_dir.clone(),
        collections_dirs: vec![collections_dir.clone(), project_root.join("collections")],
        shares_dir: project_root.join("shares"),
        project_root: Some(project_root.clone()),
        project_settings: grpctestify::serve::project::load_project_settings(&project_root).ok(),
        history_lock: tokio::sync::Mutex::new(()),
        collections_mtime: Arc::new(AtomicU64::new(0)),
    });
    serve::build_app(state)
}

/// Start a server on a random port and return the base URL
async fn start_server(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

/// Make a GET request and return JSON parsed response
async fn get_json(url: &str, path: &str) -> (u16, serde_json::Value) {
    let resp = reqwest::get(&format!("{}{}", url, path)).await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, body)
}

/// Make a POST request with JSON body and return parsed response
async fn post_json(url: &str, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let client = reqwest::Client::new();
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let uri = format!("{}{}", url, path);
    let resp = client
        .post(&uri)
        .header("content-type", "application/json")
        .body(body_str)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let resp_body: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, resp_body)
}

/// Make a PUT request with JSON body and return parsed response
async fn put_json(url: &str, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let client = reqwest::Client::new();
    let body_str = serde_json::to_string(body).unwrap_or_default();
    let uri = format!("{}{}", url, path);
    let resp = client
        .put(&uri)
        .header("content-type", "application/json")
        .body(body_str)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let resp_body: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, resp_body)
}

/// Make a DELETE request
async fn delete_req(url: &str, path: &str) -> u16 {
    let client = reqwest::Client::new();
    let uri = format!("{}{}", url, path);
    let resp = client.delete(&uri).send().await.unwrap();
    resp.status().as_u16()
}

// ─── Basic ──────────────────────────────────────────────────

#[tokio::test]
async fn test_health() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let resp = reqwest::get(&format!("{}/api/health", url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let text = resp.text().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    assert_eq!(
        body["status"], "ok",
        "health endpoint returns JSON with status=ok"
    );
}

#[tokio::test]
async fn test_version() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/version").await;
    assert_eq!(status, 200);
    assert!(!body["version"].as_str().unwrap_or("").is_empty());
}

// ─── Collections ────────────────────────────────────────────

#[tokio::test]
async fn test_list_collections() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/collections").await;
    assert_eq!(status, 200);
    let items = body.as_array().unwrap();
    assert!(!items.is_empty());
    assert!(items[0]["path"].as_str().unwrap_or("").ends_with(".gctf"));
}

#[tokio::test]
async fn test_get_collection_ok() {
    let url = start_server(test_app(PathBuf::from("."))).await;
    let (status, body) = get_json(&url, "/api/collections/examples/basic/unary.gctf").await;
    assert_eq!(status, 200);
    assert!(body["content"].as_str().unwrap_or("").contains("ENDPOINT"));
    assert!(
        body["parsed"]["endpoint"]
            .as_str()
            .unwrap_or("")
            .contains("/")
    );
}

#[tokio::test]
async fn test_get_collection_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/collections/nonexistent.gctf").await;
    assert_eq!(status, 404);
}

// ─── Save ───────────────────────────────────────────────────

#[tokio::test]
async fn test_save_and_read_back() {
    let dir = std::env::temp_dir().join("grpctestify-srv-save");
    let _ = std::fs::create_dir_all(&dir);
    let url = start_server(test_app(dir.clone())).await;

    let content = serde_json::json!({"path": "t.gctf", "content": "--- ENDPOINT ---\ntest.Svc/M\n--- REQUEST ---\n{}\n"});
    let (status, _) = post_json(&url, "/api/save", &content).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/t.gctf").await;
    assert!(
        body["content"]
            .as_str()
            .unwrap_or("")
            .contains("test.Svc/M")
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_save_structured() {
    let dir = std::env::temp_dir().join("grpctestify-srv-str");
    let _ = std::fs::create_dir_all(&dir);
    let url = start_server(test_app(dir.clone())).await;

    let req = serde_json::json!({"path":"s.gctf","endpoint":"svc.M/C","bodies":["{\"x\":1}"],"address":"h:9"});
    let (status, _) = post_json(&url, "/api/save-structured", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/s.gctf").await;
    assert_eq!(body["parsed"]["endpoint"], "svc.M/C");
    assert_eq!(body["parsed"]["address"], "h:9");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_save_traversal() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"path":"../bad.gctf","content":""});
    let (status, _) = post_json(&url, "/api/save", &req).await;
    assert_eq!(status, 404);
}

// ─── Import grpcurl ─────────────────────────────────────────

#[tokio::test]
async fn test_import_grpcurl() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"args":["grpcurl","-plaintext","-d","{\"name\":\"W\"}","h:4770","svc.G/S"]});
    let (status, body) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert_eq!(status, 200);
    assert_eq!(body["endpoint"], "svc.G/S");
    assert_eq!(body["address"], "h:4770");
    assert!(body["body"].as_str().unwrap_or("").contains("W"));
}

#[tokio::test]
async fn test_import_grpcurl_empty() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({});
    let (status, _) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert!(
        status == 400 || status == 422,
        "expected 400 or 422, got {}",
        status
    );
}

// ─── Generate grpcurl ───────────────────────────────────────

#[tokio::test]
async fn test_generate_grpcurl() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"s.C/m","body":{"k":1}});
    let (status, body) = post_json(&url, "/api/grpcurl", &req).await;
    assert_eq!(status, 200);
    assert!(body["command"].as_str().unwrap_or("").contains("grpcurl"));
    assert!(body["command"].as_str().unwrap_or("").contains("s.C/m"));
}

// ─── Call (no server) ───────────────────────────────────────

#[tokio::test]
async fn test_call_no_server() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"x.Y/z","body":{},"address":"127.0.0.1:1"});
    let (status, body) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 200);
    assert!(!body["success"].as_bool().unwrap_or(true));
    assert!(!body["error"].as_str().unwrap_or("").is_empty());
}

// ─── Edge cases ─────────────────────────────────────────────

#[tokio::test]
async fn test_save_empty_content() {
    let dir = std::env::temp_dir().join("grpctestify-srv-empty");
    let _ = std::fs::create_dir_all(&dir);
    let url = start_server(test_app(dir.clone())).await;

    let req = serde_json::json!({"path":"e.gctf","content":""});
    let (status, _) = post_json(&url, "/api/save", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/e.gctf").await;
    assert!(body["content"].as_str().unwrap_or("").is_empty());
    assert_eq!(body["parsed"]["endpoint"], "");
    assert_eq!(body["parsed"]["bodies"][0], "{}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_save_structured_no_endpoint() {
    let dir = std::env::temp_dir().join("grpctestify-srv-noep");
    let _ = std::fs::create_dir_all(&dir);
    let url = start_server(test_app(dir.clone())).await;

    let req = serde_json::json!({"path":"x.gctf","endpoint":""});
    let (status, _) = post_json(&url, "/api/save-structured", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/x.gctf").await;
    assert_eq!(body["parsed"]["endpoint"], "");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_import_grpcurl_invalid_flag() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"args":["grpcurl","--unknown-flag"]});
    let (status, _) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert_eq!(status, 400, "invalid flag should return 400");
}

#[tokio::test]
async fn test_call_missing_endpoint() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"","body":{}});
    let (status, _) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 400, "empty endpoint should be rejected");
}

// ─── Int64 precision ──────────────────────────────────────────

/// Verify that `bodies_raw` is accepted and parsed correctly.
#[tokio::test]
async fn test_execute_call_uses_bodies_raw() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;

    // Send raw JSON via reqwest to avoid serde_json! macro truncation
    let client = reqwest::Client::new();
    let body_str = r#"{"endpoint":"svc.M/m","bodies_raw":["{\"id\":18446744073709551615}","{\"id\":-9223372036854775808}"],"address":"127.0.0.1:1"}"#;
    let uri = format!("{}/api/call", url);
    let resp = client
        .post(&uri)
        .header("content-type", "application/json")
        .body(body_str.to_string())
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

    assert_eq!(
        status, 200,
        "bodies_raw should be accepted, got {}: {}",
        status, text
    );
    assert!(
        !body["error"].as_str().unwrap_or("").is_empty(),
        "expected connection error"
    );
}

/// Test that serde_json can round-trip u64::MAX through a raw string.
#[test]
fn test_serde_json_u64_roundtrip() {
    let raw = r#"{"id":18446744073709551615}"#;
    let val: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(val["id"].as_u64(), Some(18446744073709551615u64));
    // Round-trip back to string
    let back = serde_json::to_string(&val).unwrap();
    assert!(
        back.contains("18446744073709551615"),
        "u64::MAX preserved in round-trip: {}",
        back
    );
}

/// Test that JavaScript-style truncation does NOT happen on our backend.
#[test]
fn test_no_javascript_truncation() {
    // JavaScript would truncate 18446744073709551615 to 18446744073709552000
    let raw = r#"{"id":18446744073709551615}"#;
    let val: serde_json::Value = serde_json::from_str(raw).unwrap();
    let back = serde_json::to_string(&val).unwrap();
    // Ensure the exact value is preserved, not a truncated version
    assert!(!back.contains("18446744073709552000"), "no JS truncation");
    assert_eq!(val["id"].as_u64(), Some(18446744073709551615u64));
}

/// Test that serde_json in Rust preserves int64 from raw JSON strings.
#[test]
fn test_serde_json_preserves_int64() {
    // This is a compile-time + runtime test: serde_json should preserve u64::MAX
    let raw = r#"{"id":18446744073709551615}"#;
    let val: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        val["id"].as_u64(),
        Some(18446744073709551615u64),
        "u64::MAX preserved"
    );

    let raw2 = r#"{"id":-9223372036854775808}"#;
    let val2: serde_json::Value = serde_json::from_str(raw2).unwrap();
    assert_eq!(val2["id"].as_i64(), Some(i64::MIN), "i64::MIN preserved");

    let raw3 = r#"{"id":9223372036854775807}"#;
    let val3: serde_json::Value = serde_json::from_str(raw3).unwrap();
    assert_eq!(val3["id"].as_i64(), Some(i64::MAX), "i64::MAX preserved");
}

// ─── Project mode ───────────────────────────────────────────

/// Helper to set up a temporary .grpctestify directory
fn setup_project_dir(label: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("grpctestify-project-{}-{}", label, ts));
    let _ = std::fs::remove_dir_all(&dir);
    project::init_project_dir(&dir).expect("init_project_dir should succeed");
    dir
}

#[tokio::test]
async fn test_project_info_active() {
    let dir = setup_project_dir("info");
    let url = start_server(test_app_project(dir.clone())).await;

    let (status, body) = get_json(&url, "/api/project/info").await;
    assert_eq!(status, 200);
    assert_eq!(body["active"], true, "project mode should be active");
    assert!(body["envs"].is_array(), "envs should be an array");
    assert!(
        !body["project_dir"].as_str().unwrap_or("").is_empty(),
        "project_dir should be set"
    );
}

#[tokio::test]
async fn test_project_settings_get() {
    let dir = setup_project_dir("settings-get");
    let url = start_server(test_app_project(dir.clone())).await;

    let (status, body) = get_json(&url, "/api/project/settings").await;
    assert_eq!(status, 200);
    assert_eq!(body["address"], "localhost:4770");
    assert_eq!(body["protocol"], "grpc");
    assert_eq!(body["tls"], false);
    assert_eq!(body["tls_insecure"], true);
    assert_eq!(body["active_env"], "example");
}

#[tokio::test]
async fn test_project_settings_put() {
    let dir = setup_project_dir("settings-put");
    let url = start_server(test_app_project(dir.clone())).await;

    let update = serde_json::json!({
        "address": "custom:4771",
        "protocol": "grpc-web",
        "tls": true,
        "tls_insecure": false,
        "active_env": null,
    });
    let (status, _) = put_json(&url, "/api/project/settings", &update).await;
    assert_eq!(status, 200);

    // Verify the update persisted
    let (_, body) = get_json(&url, "/api/project/settings").await;
    assert_eq!(body["address"], "custom:4771");
    assert_eq!(body["protocol"], "grpc-web");
    assert_eq!(body["tls"], true);
    assert_eq!(body["tls_insecure"], false);
}

#[tokio::test]
async fn test_project_env_list_with_example() {
    let dir = setup_project_dir("env-list");
    let url = start_server(test_app_project(dir.clone())).await;

    let (status, body) = get_json(&url, "/api/project/env/list").await;
    assert_eq!(status, 200);
    // .env.example is created by init, which IS listed (it's a valid env)
    assert!(
        body.as_array()
            .unwrap_or(&vec![])
            .contains(&serde_json::Value::String("example".into())),
        "env list should contain 'example' from .env.example"
    );
}

#[tokio::test]
async fn test_project_env_crud() {
    let dir = setup_project_dir("env-crud");
    let url = start_server(test_app_project(dir.clone())).await;

    // Create env file
    let content = serde_json::json!({"content": "GRPC_ADDRESS=test:4770\nAPI_KEY=test123\n"});
    let (status, _) = put_json(&url, "/api/project/env/staging", &content).await;
    assert_eq!(status, 200);

    // List should show it
    let (_, body) = get_json(&url, "/api/project/env/list").await;
    let envs = body.as_array().unwrap();
    assert!(
        envs.contains(&serde_json::Value::String("staging".into())),
        "env list should contain 'staging'"
    );

    // Read back
    let (_, body) = get_json(&url, "/api/project/env/staging").await;
    let raw: String = serde_json::from_value(body).unwrap_or_default();
    assert!(
        raw.contains("API_KEY=test123"),
        "env content should contain API_KEY"
    );

    // Create local overrides
    let local = serde_json::json!({"content": "API_KEY=local-secret\n"});
    let (status, _) = put_json(&url, "/api/project/env/staging/local", &local).await;
    assert_eq!(status, 200);

    // Read local overrides
    let (_, body) = get_json(&url, "/api/project/env/staging/local").await;
    assert_eq!(body["exists"], true);
    let local_content: String = serde_json::from_value(body["content"].clone()).unwrap_or_default();
    assert!(local_content.contains("local-secret"));

    // Delete local overrides
    let status = delete_req(&url, "/api/project/env/staging/local").await;
    assert_eq!(status, 200);

    // Verify deleted
    let (_, body) = get_json(&url, "/api/project/env/staging/local").await;
    assert_eq!(body["exists"], false);
}

#[tokio::test]
async fn test_project_info_not_active_without_project() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/project/info").await;
    assert_eq!(status, 200);
    assert_eq!(
        body["active"], false,
        "without .grpctestify project should be inactive"
    );
}

#[tokio::test]
async fn test_project_settings_get_without_project_returns_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/project/settings").await;
    assert_eq!(status, 404, "settings should 404 without project");
}

#[tokio::test]
async fn test_project_env_list_without_project_returns_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/project/env/list").await;
    assert_eq!(status, 404, "env list should 404 without project");
}

#[tokio::test]
async fn test_project_create_directory_and_move() {
    let dir = setup_project_dir("dir-move");
    let url = start_server(test_app_project(dir.clone())).await;

    // Create a subdirectory
    let client = reqwest::Client::new();
    let dir_uri = format!("{}/api/dir/subdir", url);
    let resp = client.post(&dir_uri).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 200, "create directory");

    // Create a file in that dir
    let content = serde_json::json!({"path": "subdir/test.gctf", "content": "--- ENDPOINT ---\ntest.Svc/M\n--- REQUEST ---\n{}\n"});
    let (status, _) = post_json(&url, "/api/save", &content).await;
    assert_eq!(status, 200, "save file in subdir");

    // Move the file
    let move_req = serde_json::json!({"from": "subdir/test.gctf", "to": "moved.gctf"});
    let move_uri = format!("{}/api/move", url);
    let resp = client
        .post(&move_uri)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&move_req).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "move file");

    // Verify the moved file exists at new location
    let (status, _) = get_json(&url, "/api/collections/moved.gctf").await;
    assert_eq!(status, 200, "moved file readable");

    // Delete the file
    let delete_uri = format!("{}/api/collections/moved.gctf", url);
    let resp = client.delete(&delete_uri).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 200, "delete file");

    // Verify deletion
    let (status, _) = get_json(&url, "/api/collections/moved.gctf").await;
    assert_eq!(status, 404, "deleted file should 404");
}
