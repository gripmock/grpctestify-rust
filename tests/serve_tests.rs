#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#[path = "support/mod.rs"]
mod support;

/// The echo server is the only reference service with a client-streaming
/// method, and it is compiled under this feature — the CI matrix runs with
/// `--all-features`.
#[cfg(feature = "test-servers")]
#[path = "servers/servers.rs"]
mod servers;

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
        write_lock: tokio::sync::Mutex::new(()),
        collections_mtime: Arc::new(AtomicU64::new(0)),
        jobs: Default::default(),
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
        write_lock: tokio::sync::Mutex::new(()),
        collections_mtime: Arc::new(AtomicU64::new(0)),
        jobs: Default::default(),
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
async fn health() {
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
async fn version() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/version").await;
    assert_eq!(status, 200);
    assert!(!body["version"].as_str().unwrap_or("").is_empty());
}

// ─── Collections ────────────────────────────────────────────

#[tokio::test]
async fn list_collections() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/collections").await;
    assert_eq!(status, 200);
    let items = body.as_array().unwrap();
    assert!(!items.is_empty());
    // Directories sort before files (`examples/scripts/` holds `.rhai`
    // plugin examples, no `.gctf` — it's a legitimate "empty" dir from the
    // playground's point of view, so item 0 isn't guaranteed to be a file).
    assert!(
        items
            .iter()
            .any(|i| i["path"].as_str().unwrap_or("").ends_with(".gctf")),
        "at least one .gctf collection must be listed: {items:?}"
    );
}

#[tokio::test]
async fn get_collection_ok() {
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
async fn get_collection_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/collections/nonexistent.gctf").await;
    assert_eq!(status, 404);
}

// ─── Save ───────────────────────────────────────────────────

#[tokio::test]
async fn save_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

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
}

#[tokio::test]
async fn save_structured() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({"path":"s.gctf","endpoint":"svc.M/C","bodies":["{\"x\":1}"],"address":"h:9"});
    let (status, _) = post_json(&url, "/api/save-structured", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/s.gctf").await;
    assert_eq!(body["parsed"]["endpoint"], "svc.M/C");
    assert_eq!(body["parsed"]["address"], "h:9");
}

/// Creating a file is not editing one: `new file` sent a plain save, so a name
/// that already existed overwrote the test that had it.
#[tokio::test]
async fn create_only_refuses_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let first = serde_json::json!({
        "path": "t.gctf",
        "content": "--- ENDPOINT ---\ntest.Svc/M\n",
        "create_only": true,
    });
    let (status, _) = post_json(&url, "/api/save", &first).await;
    assert_eq!(status, 200);

    let second = serde_json::json!({"path": "t.gctf", "content": "gone\n", "create_only": true});
    let (status, _) = post_json(&url, "/api/save", &second).await;
    assert_eq!(status, 409);

    let (_, body) = get_json(&url, "/api/collections/t.gctf").await;
    assert!(
        body["content"]
            .as_str()
            .unwrap_or("")
            .contains("test.Svc/M")
    );
}

/// An echo that answers with the method, the body and the headers it was sent —
/// enough to prove the workbench dials the way a run does.
fn echo_app() -> Router {
    use axum::extract::Request;
    use axum::routing::any;
    Router::new().route(
        "/echo",
        any(|req: Request| async move {
            let method = req.method().to_string();
            let headers: std::collections::HashMap<String, String> = req
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let bytes = axum::body::to_bytes(req.into_body(), 64 * 1024)
                .await
                .unwrap_or_default();
            axum::Json(serde_json::json!({
                "method": method,
                "headers": headers,
                "body": String::from_utf8_lossy(&bytes),
            }))
        }),
    )
}

/// Execute dropped a body of `{}` and a run sent it, so the workbench and the
/// file it saves made different calls — with the default body of a new tab.
#[tokio::test]
async fn execute_sends_the_body_a_run_would_send() {
    let echo = start_server(echo_app()).await;
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({
        "endpoint": "POST /echo",
        "address": echo,
        "bodies_raw": ["{}"],
    });
    let (status, body) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 200);
    let answered = &body["messages"][0];
    assert_eq!(answered["body"], "{}");
    assert_eq!(answered["headers"]["content-type"], "application/json");
}

/// The address field's own badge says the file wins; for an HTTP file the field
/// won, so Execute dialled somewhere a run of that file never would.
#[tokio::test]
async fn an_http_file_dials_its_own_address() {
    let echo = start_server(echo_app()).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("e.httf"),
        format!("--- ADDRESS ---\n{echo}\n\n--- ENDPOINT ---\nPOST /echo\n"),
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({
        "endpoint": "POST /echo",
        "address": "http://127.0.0.1:1",
        "collection_path": "e.httf",
        "bodies_raw": ["{\"a\":1}"],
    });
    let (status, body) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 200);
    assert_eq!(body["error"], serde_json::Value::Null);
    assert_eq!(body["messages"][0]["body"], "{\"a\":1}");
}

/// Cancel read the flag between files, so cancelling a run of one slow test did
/// nothing until that test finished on its own.
#[tokio::test]
async fn cancel_stops_the_call_in_flight() {
    use axum::routing::get;
    let slow = start_server(Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            "late"
        }),
    ))
    .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("slow.httf"),
        format!("--- ADDRESS ---\n{slow}\n\n--- ENDPOINT ---\nGET /slow\n\n--- ASSERTS ---\n@status() == 200\n"),
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let (status, job) = post_json(
        &url,
        "/api/jobs",
        &serde_json::json!({ "paths": ["slow.httf"] }),
    )
    .await;
    assert_eq!(status, 200);
    let id = job["id"].as_str().unwrap().to_string();

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (status, _) = post_json(
        &url,
        &format!("/api/jobs/{id}/cancel"),
        &serde_json::json!({}),
    )
    .await;
    assert_eq!(status, 200);

    // The run ends because the call was dropped, not because the server
    // eventually answered thirty seconds later.
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (_, summary) = get_json(&url, &format!("/api/jobs/{id}")).await;
            if summary["status"] != "running" {
                return summary;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the run should end as soon as it is cancelled");

    assert_eq!(ended["status"], "cancelled");
}

/// An HTTP body is whatever the server takes. The structured save refused
/// anything that was not JSON, so a form or an XML request could be run by the
/// CLI and not written by the workbench.
#[tokio::test]
async fn an_http_request_saves_a_body_that_is_not_json() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({
        "path": "form.httf",
        "endpoint": "POST /submit",
        "bodies": ["name=Ada&age=36"],
        "asserts": ["@status() == 200"],
    });
    let (status, _) = post_json(&url, "/api/save-structured", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/form.httf").await;
    let content = body["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("--- REQUEST ---\nname=Ada&age=36"),
        "{content}"
    );

    // And it comes back: read as a request with no body, the next save would
    // have written the file without it.
    assert_eq!(body["parsed"]["bodies"][0], "name=Ada&age=36");

    // A gRPC message still has to be a message.
    let bad = serde_json::json!({"path": "m.gctf", "endpoint": "a.B/C", "bodies": ["not json"]});
    let (status, _) = post_json(&url, "/api/save-structured", &bad).await;
    assert_eq!(status, 400);
}

/// A client-streaming request written as one block of JSON documents was read
/// as a request with no messages at all, so the workbench showed an empty body
/// and the next save wrote a call with nothing to send.
#[tokio::test]
async fn a_stream_written_as_one_block_is_read_as_its_messages() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("chunks.gctf"),
        "--- ENDPOINT ---\na.B/Upload\n\n--- REQUEST ---\n{\"chunk\": 1}\n{\"chunk\": 2}\n\n--- ASSERTS ---\n.ok\n",
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let (_, body) = get_json(&url, "/api/collections/chunks.gctf").await;
    let bodies = body["parsed"]["bodies"].as_array().expect("bodies");
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert!(
        bodies[1]
            .as_str()
            .unwrap_or_default()
            .contains("\"chunk\": 2"),
        "{bodies:?}"
    );
}

/// A run of an HTTP file reported `0` — the gRPC "OK" — so the panel showed no
/// status for a file the rail had just run.
#[tokio::test]
async fn running_an_http_file_reports_the_status_it_answered_with() {
    let echo = start_server(echo_app()).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("e.httf"),
        format!("--- ADDRESS ---\n{echo}\n\n--- ENDPOINT ---\nPOST /echo\n\n--- ASSERTS ---\n@status() == 200\n"),
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let (status, body) = post_json(
        &url,
        "/api/run",
        &serde_json::json!({"collection_path": "e.httf"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["success"], true, "{body}");
    assert_eq!(body["grpc_status"], 200, "{body}");
}

/// One suite, two families, one code path. A mixed selection has to run as one
/// job — the same runner, the same events, the same report — because that is
/// the claim `.httf` was added under: an adapter, not a second workbench.
#[tokio::test]
async fn a_mixed_suite_runs_as_one_job() {
    let echo = start_server(echo_app()).await;
    let grpc = support::spawn_health_server().await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("http.httf"),
        format!("--- ADDRESS ---\n{echo}\n\n--- ENDPOINT ---\nPOST /echo\n\n--- REQUEST ---\n{{\"a\": 1}}\n\n--- ASSERTS ---\n@status() == 200\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("grpc.gctf"),
        format!("--- ADDRESS ---\n{grpc}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"),
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let (status, job) = post_json(
        &url,
        "/api/jobs",
        &serde_json::json!({ "paths": ["http.httf", "grpc.gctf"], "reports": ["json"] }),
    )
    .await;
    assert_eq!(status, 200);
    let id = job["id"].as_str().unwrap().to_string();

    let ended = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let (_, summary) = get_json(&url, &format!("/api/jobs/{id}")).await;
            if summary["status"] != "running" {
                return summary;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the run ends");

    assert_eq!(ended["status"], "passed", "{ended}");
    assert_eq!(
        (ended["passed"].as_u64(), ended["failed"].as_u64()),
        (Some(2), Some(0)),
        "{ended}"
    );

    /* Both files reported through the same event stream, by name. */
    let events = ended["events"].as_array().expect("events");
    let passed: Vec<&str> = events
        .iter()
        .filter(|e| e["event"] == "test_pass")
        .filter_map(|e| e["testId"].as_str())
        .collect();
    assert!(passed.contains(&"http.httf"), "{passed:?}");
    assert!(passed.contains(&"grpc.gctf"), "{passed:?}");

    /* And each says what its answer came back with, so a run started from the
    rail tells the panel what a run started from the panel tells it. */
    let status_of = |id: &str| {
        events
            .iter()
            .find(|e| e["testId"] == id && e["event"] == "test_pass")
            .and_then(|e| e["grpcStatus"].as_u64())
    };
    assert_eq!(status_of("http.httf"), Some(200), "{events:?}");
    assert_eq!(status_of("grpc.gctf"), Some(0), "{events:?}");

    /* And one report holds both, each saying which family it belongs to — a
    dashboard reading it can tell them apart without parsing file names. */
    let report = ended["reports"]
        .as_array()
        .and_then(|files| files.iter().find_map(|f| f.as_str()))
        .expect("the run wrote a report");
    let (status, json) = get_json(&url, &format!("/api/jobs/{id}/report/{report}")).await;
    assert_eq!(status, 200);
    let families: Vec<(&str, &str)> = json["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter_map(|r| Some((r["name"].as_str()?, r["family"].as_str()?)))
        .collect();
    assert!(
        families
            .iter()
            .any(|(name, family)| name.ends_with("http.httf") && *family == "httf"),
        "{families:?}",
    );
    assert!(
        families
            .iter()
            .any(|(name, family)| name.ends_with("grpc.gctf") && *family == "gctf"),
        "{families:?}",
    );
}

/// The load runner dials gRPC. Benching an `.httf` sent a hundred gRPC requests
/// at an HTTP server and reported the target "likely misconfigured or
/// unreachable" — which it was not.
#[tokio::test]
async fn a_bench_of_an_http_file_is_refused_in_those_words() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("p.httf"),
        "--- ADDRESS ---\nhttps://api.example.com\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("g.gctf"),
        "--- ADDRESS ---\nlocalhost:4770\n\n--- ENDPOINT ---\na.B/C\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok\n",
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let (status, body) = post_json(
        &url,
        "/api/jobs",
        &serde_json::json!({"paths": ["p.httf"], "kind": "bench"}),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // A mixed selection measures the files it can, and says which those are.
    let (status, body) = post_json(
        &url,
        "/api/jobs",
        &serde_json::json!({"paths": ["p.httf", "g.gctf"], "kind": "bench"}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["paths"], serde_json::json!(["g.gctf"]), "{body}");
    assert_eq!(body["total"], 1, "{body}");
}

/// grpc-web and ConnectRPC carry no reflection: the answer was an empty list
/// and an empty error, so "explore my API" failed in silence for two of the
/// three transports the workbench offers.
#[tokio::test]
async fn reflection_over_a_transport_that_has_none_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    for protocol in ["grpc-web", "connectrpc"] {
        let (status, body) = post_json(
            &url,
            "/api/reflect",
            &serde_json::json!({ "address": "127.0.0.1:1", "protocol": protocol }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["services"].as_array().map(Vec::len), Some(0), "{body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("no reflection"),
            "{protocol}: {body}"
        );
    }
}

#[tokio::test]
async fn save_traversal() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"path":"../bad.gctf","content":""});
    let (status, _) = post_json(&url, "/api/save", &req).await;
    assert_eq!(status, 404);
}

// ─── Import grpcurl ─────────────────────────────────────────

#[tokio::test]
async fn import_grpcurl() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"args":["grpcurl","-plaintext","-d","{\"name\":\"W\"}","h:4770","svc.G/S"]});
    let (status, body) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert_eq!(status, 200);
    assert_eq!(body["endpoint"], "svc.G/S");
    assert_eq!(body["address"], "h:4770");
    assert!(body["body"].as_str().unwrap_or("").contains("W"));
}

/// The command's schema, its certificates and its timeout were parsed and then
/// dropped on the way out, so importing a call that dialled with a client
/// certificate produced one that does not.
#[tokio::test]
async fn import_grpcurl_carries_the_whole_command() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"args":[
        "grpcurl",
        "-cacert", "/etc/ca.pem",
        "-cert", "/etc/client.pem",
        "-key", "/etc/client.key",
        "-proto", "auth.proto",
        "-import-path", "./proto",
        "-max-time", "12",
        "-gzip",
        "-H", "x-api-key: abc",
        "h:4770", "svc.G/S",
    ]});
    let (status, body) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert_eq!(status, 200);
    assert_eq!(body["tls"]["ca_cert"], "/etc/ca.pem");
    assert_eq!(body["tls"]["client_cert"], "/etc/client.pem");
    assert_eq!(body["tls"]["client_key"], "/etc/client.key");
    assert_eq!(body["proto"]["files"], "auth.proto");
    assert_eq!(body["proto"]["import_paths"], "./proto");
    assert_eq!(body["options"]["max-time"], "12");
    assert_eq!(body["options"]["compression"], "gzip");
    assert_eq!(body["headers"]["x-api-key"], "abc");
}

#[tokio::test]
async fn import_grpcurl_empty() {
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
async fn generate_grpcurl() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"s.C/m","body":{"k":1}});
    let (status, body) = post_json(&url, "/api/grpcurl", &req).await;
    assert_eq!(status, 200);
    assert!(body["command"].as_str().unwrap_or("").contains("grpcurl"));
    assert!(body["command"].as_str().unwrap_or("").contains("s.C/m"));
}

// ─── Call (no server) ───────────────────────────────────────

#[tokio::test]
async fn call_no_server() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"x.Y/z","body":{},"address":"127.0.0.1:1"});
    let (status, body) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 200);
    assert!(!body["success"].as_bool().unwrap_or(true));
    assert!(!body["error"].as_str().unwrap_or("").is_empty());
}

// ─── Edge cases ─────────────────────────────────────────────

#[tokio::test]
async fn save_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({"path":"e.gctf","content":""});
    let (status, _) = post_json(&url, "/api/save", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/e.gctf").await;
    assert!(body["content"].as_str().unwrap_or("").is_empty());
    assert_eq!(body["parsed"]["endpoint"], "");
    assert_eq!(body["parsed"]["bodies"][0], "{}");
}

#[tokio::test]
async fn save_structured_no_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({"path":"x.gctf","endpoint":""});
    let (status, _) = post_json(&url, "/api/save-structured", &req).await;
    assert_eq!(status, 200);

    let (_, body) = get_json(&url, "/api/collections/x.gctf").await;
    assert_eq!(body["parsed"]["endpoint"], "");
}

#[tokio::test]
async fn import_grpcurl_invalid_flag() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"args":["grpcurl","--unknown-flag"]});
    let (status, _) = post_json(&url, "/api/import-grpcurl", &req).await;
    assert_eq!(status, 400, "invalid flag should return 400");
}

#[tokio::test]
async fn call_missing_endpoint() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"endpoint":"","body":{}});
    let (status, _) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 400, "empty endpoint should be rejected");
}

// ─── Int64 precision ──────────────────────────────────────────

/// Verify that `bodies_raw` is accepted and parsed correctly.
#[tokio::test]
async fn execute_call_uses_bodies_raw() {
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
fn serde_json_u64_roundtrip() {
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
fn no_javascript_truncation() {
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
fn serde_json_preserves_int64() {
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
async fn project_info_active() {
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
async fn project_settings_get() {
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
async fn project_settings_put() {
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
async fn project_env_list_with_example() {
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
async fn project_env_crud() {
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
async fn project_info_not_active_without_project() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, body) = get_json(&url, "/api/project/info").await;
    assert_eq!(status, 200);
    assert_eq!(
        body["active"], false,
        "without .grpctestify project should be inactive"
    );
}

#[tokio::test]
async fn project_settings_get_without_project_returns_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/project/settings").await;
    assert_eq!(status, 404, "settings should 404 without project");
}

#[tokio::test]
async fn project_env_list_without_project_returns_404() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let (status, _) = get_json(&url, "/api/project/env/list").await;
    assert_eq!(status, 404, "env list should 404 without project");
}

#[tokio::test]
async fn project_create_directory_and_move() {
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

// ─── /api/run — full ASSERTS evaluation via the shared runner ─────────

#[tokio::test]
async fn run_passing_gctf_reports_success_and_assertion_detail() {
    let address = support::spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pass.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let req = serde_json::json!({"collection_path": "pass.gctf"});
    let (status, body) = post_json(&url, "/api/run", &req).await;

    assert_eq!(status, 200);
    assert!(body["success"].as_bool().unwrap_or(false), "{body:#}");
    assert_eq!(body["assertions"][0]["passed"], true);
    assert_eq!(
        body["assertions"][0]["expression"],
        ".status == \"SERVING\""
    );
}

/// Regression: the ASSERTS stream handler consumed messages without pushing
/// them into `captured_response`, so ASSERTS-only tests returned zero
/// `response_messages` and the playground showed "No response messages".
#[tokio::test]
async fn run_asserts_only_gctf_returns_response_messages() {
    let address = support::spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("msgs.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let req = serde_json::json!({"collection_path": "msgs.gctf"});
    let (status, body) = post_json(&url, "/api/run", &req).await;

    assert_eq!(status, 200);
    assert!(body["success"].as_bool().unwrap_or(false), "{body:#}");
    assert_eq!(
        body["response_messages"],
        serde_json::json!([{"status": "SERVING"}]),
        "{body:#}"
    );
}

#[tokio::test]
async fn proto_source_renders_service_schema_via_reflection() {
    let address = support::spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let req = serde_json::json!({
        "address": address,
        "endpoint": "grpc.health.v1.Health/Check",
        "body": {},
    });
    let (status, body) = post_json(&url, "/api/proto-source", &req).await;

    assert_eq!(status, 200);
    let source = body["source"].as_str().unwrap_or_default();
    assert!(source.contains("service grpc.health.v1.Health"), "{body:#}");
    assert!(
        source.contains("rpc Check(HealthCheckRequest) returns (HealthCheckResponse);"),
        "{body:#}"
    );
    assert!(source.contains("message HealthCheckRequest"), "{body:#}");
    assert!(source.contains("ServingStatus status = 1;"), "{body:#}");
}

#[tokio::test]
async fn run_failing_gctf_reports_expected_and_actual() {
    let address = support::spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("fail.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"NOT_SERVING\"\n"
        ),
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let req = serde_json::json!({"collection_path": "fail.gctf"});
    let (status, body) = post_json(&url, "/api/run", &req).await;

    assert_eq!(status, 200);
    assert!(!body["success"].as_bool().unwrap_or(true), "{body:#}");
    assert_eq!(body["assertions"][0]["passed"], false);
    assert!(
        body["assertions"][0]["actual"]
            .as_str()
            .unwrap_or("")
            .contains("SERVING")
    );
}

#[tokio::test]
async fn run_with_session_id_appends_project_history() {
    let address = support::spawn_health_server().await;
    let dir = setup_project_dir("run-history");
    let collections_dir = dir.join(".grpctestify").join("collections");
    std::fs::write(
        collections_dir.join("pass.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .unwrap();

    let url = start_server(test_app_project(dir.clone())).await;
    let req = serde_json::json!({"collection_path": "pass.gctf", "session_id": "test-session"});
    let (status, body) = post_json(&url, "/api/run", &req).await;
    assert_eq!(status, 200);
    assert!(body["success"].as_bool().unwrap_or(false), "{body:#}");

    let (hist_status, hist_body) = get_json(&url, "/api/project/history").await;
    assert_eq!(hist_status, 200);
    let entries = hist_body["test-session"]
        .as_array()
        .expect("test-session history should exist");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "run");
    assert_eq!(entries[0]["collection_path"], "pass.gctf");
    assert_eq!(entries[0]["response"]["status"], "ok");
    assert_eq!(entries[0]["response"]["assertions_passed"], 1);

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── Diagnostics (unsaved editor content, no file I/O) ─────────

#[tokio::test]
async fn diagnostics_reports_optimizer_finding() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({
        "content": "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n!!@has_header(\"x\")\n"
    });
    let (status, body) = post_json(&url, "/api/diagnostics", &req).await;
    assert_eq!(status, 200);
    let diags = body.as_array().expect("diagnostics should be an array");
    assert!(
        diags
            .iter()
            .any(|d| d["code"].as_str().unwrap_or("").contains("C001")),
        "the first step offered for `!!x` is its canonical spelling: {body:#}"
    );
}

#[tokio::test]
async fn diagnostics_clean_file_reports_nothing() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({
        "content": "--- ADDRESS ---\nlocalhost:50051\n\n--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"ok\"\n"
    });
    let (status, body) = post_json(&url, "/api/diagnostics", &req).await;
    assert_eq!(status, 200);
    let diags = body.as_array().expect("diagnostics should be an array");
    assert!(diags.is_empty(), "{body:#}");
}

#[tokio::test]
async fn run_missing_file_404s() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"collection_path": "does-not-exist.gctf"});
    let (status, _) = post_json(&url, "/api/run", &req).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn run_rejects_path_traversal() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({"collection_path": "../../../etc/passwd"});
    let (status, _) = post_json(&url, "/api/run", &req).await;
    assert_eq!(status, 404);
}

// ─── mTLS fields (--tls-cert/--tls-key equivalents) ────────────

/// A nonexistent-but-set cert/key path pair must reach the real "Failed to
/// read client certificate" error from the tonic layer — proving
/// `tls_cert`/`tls_key` actually flow through to `TlsConfig`, not silently
/// dropped the way they were before (`client_cert_path: None` was
/// hardcoded at every playground call site).
#[tokio::test]
async fn execute_call_honors_client_cert_fields() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({
        "endpoint": "x.Y/z",
        "body": {},
        "address": "127.0.0.1:1",
        "tls": true,
        "tls_insecure": true,
        "tls_cert": "/nonexistent/cert.pem",
        "tls_key": "/nonexistent/key.pem",
    });
    let (status, body) = post_json(&url, "/api/call", &req).await;
    assert_eq!(status, 200);
    assert!(!body["success"].as_bool().unwrap_or(true));
    let error = body["error"].as_str().unwrap_or("");
    assert!(
        error.contains("client certificate"),
        "expected the client-cert-path to have actually been read (and fail, since it doesn't exist): {error}"
    );
}

#[tokio::test]
async fn reflect_honors_client_cert_fields() {
    let url = start_server(test_app(PathBuf::from("examples"))).await;
    let req = serde_json::json!({
        "address": "127.0.0.1:1",
        "tls": true,
        "tls_insecure": true,
        "tls_cert": "/nonexistent/cert.pem",
        "tls_key": "/nonexistent/key.pem",
    });
    let (status, body) = post_json(&url, "/api/reflect", &req).await;
    assert_eq!(status, 200);
    let error = body["error"].as_str().unwrap_or("");
    assert!(
        error.contains("client certificate"),
        "expected the client-cert-path to have actually been read: {error}"
    );
}

/// A client-streaming method is one call carrying every message. `/api/call`
/// used to send one RPC per message, so a three-message request became three
/// calls the server saw as three separate one-message streams — the panel said
/// "client streaming" and the wire disagreed.
#[cfg(feature = "test-servers")]
#[tokio::test]
async fn a_client_streaming_call_carries_every_message_in_one_rpc() {
    let address = servers::echo::spawn_echo_server_with_reflection().await;
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({
        "address": address,
        "endpoint": "echo.EchoService/Repeat",
        "bodies_raw": [
            "{\"message\":\"one\"}",
            "{\"message\":\"two\"}",
            "{\"message\":\"three\"}"
        ],
    });
    let (status, body) = post_json(&url, "/api/call", &req).await;

    assert_eq!(status, 200, "{body:#}");
    assert_eq!(body["success"], true, "{body:#}");
    assert_eq!(
        body["shape"], "client",
        "the schema names the shape: {body:#}"
    );
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "one call, one response: {body:#}");
    assert_eq!(
        messages[0]["total_messages"], 3,
        "the server saw all three on one stream: {body:#}"
    );
}

/// The counterpart: a unary method with several messages stays several calls,
/// which is what the panel says it does.
#[cfg(feature = "test-servers")]
#[tokio::test]
async fn several_messages_to_a_unary_method_stay_several_calls() {
    let address = servers::echo::spawn_echo_server_with_reflection().await;
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let req = serde_json::json!({
        "address": address,
        "endpoint": "echo.EchoService/Echo",
        "bodies_raw": ["{\"text\":\"a\"}", "{\"text\":\"b\"}"],
    });
    let (status, body) = post_json(&url, "/api/call", &req).await;

    assert_eq!(status, 200, "{body:#}");
    assert_eq!(body["shape"], "unary", "{body:#}");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "one response per call: {body:#}");
}

/// A compiled descriptor set is bytes: it arrives base64 and must land on disk
/// byte-identical, or the schema it carries is unreadable.
#[tokio::test]
async fn a_descriptor_set_uploads_as_base64_and_lands_intact() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let raw: Vec<u8> = vec![0x0a, 0x00, 0xff, 0x7f, 0x41];
    let req = serde_json::json!({
        "filename": "schema.pb",
        "encoding": "base64",
        "content": "CgD/f0E=",
    });
    let (status, _) = post_json(&url, "/api/proto-upload", &req).await;
    assert_eq!(status, 200);
    assert_eq!(std::fs::read(dir.path().join("schema.pb")).unwrap(), raw);

    let (status, body) = post_json(
        &url,
        "/api/proto-upload",
        &serde_json::json!({ "filename": "notes.txt", "content": "x" }),
    )
    .await;
    assert_eq!(status, 400, "{body:#}");
}

/// The picker lists what the project holds, including the protos it keeps in a
/// subdirectory — a flat listing showed nothing for most real projects.
#[tokio::test]
async fn proto_files_lists_sources_and_descriptors_under_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("proto/v1")).unwrap();
    std::fs::write(
        dir.path().join("proto/v1/auth.proto"),
        "syntax = \"proto3\";\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("schema.desc"), [0u8, 1, 2]).unwrap();
    std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let (status, body) = get_json(&url, "/api/proto-files").await;

    assert_eq!(status, 200);
    let listed: Vec<(String, String)> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_string(),
                f["kind"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            ("proto/v1/auth.proto".to_string(), "proto".to_string()),
            ("schema.desc".to_string(), "descriptor".to_string()),
        ],
        "{body:#}"
    );
}

/// `META.links` was read out of a file and dropped by the next save: four of
/// the section's five fields survived and the fifth silently went.
#[tokio::test]
async fn a_save_keeps_the_links_the_file_had() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("one.gctf"),
        "--- ENDPOINT ---\na.A/One\n\n--- META ---\nname: login\nlinks: [https://jira/AUTH-1, https://runbook]\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let (status, body) = get_json(&url, "/api/collections/one.gctf").await;
    assert_eq!(status, 200, "{body:#}");
    let parsed = &body["parsed"];
    assert_eq!(
        parsed["meta_links"],
        serde_json::json!(["https://jira/AUTH-1", "https://runbook"]),
        "the links are read: {body:#}"
    );

    // Save it back the way the workbench does: carrying the meta it just read.
    let save = serde_json::json!({
        "path": "one.gctf",
        "original_path": "one.gctf",
        "endpoint": "a.A/One",
        "bodies": ["{}"],
        "asserts": [".ok == true"],
        "meta": {
            "name": parsed["meta_name"],
            "tags": parsed["meta_tags"],
            "links": parsed["meta_links"],
        },
    });
    let (status, body) = post_json(&url, "/api/save-structured", &save).await;
    assert_eq!(status, 200, "{body:#}");

    let written = std::fs::read_to_string(dir.path().join("one.gctf")).unwrap();
    assert!(
        written.contains("https://jira/AUTH-1"),
        "links survive the save: {written}"
    );
    assert!(
        written.contains("https://runbook"),
        "both of them: {written}"
    );
}

/// Chain edits are read-modify-write of one file. Without a lock two of them
/// interleave and a read lands inside another's write — which produced a file
/// whose steps had lost their section headers.
#[tokio::test]
async fn concurrent_chain_edits_do_not_tear_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("chain.gctf"),
        "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
    )
    .unwrap();

    let url = start_server(test_app(dir.path().to_path_buf())).await;
    let body = serde_json::json!({ "path": "chain.gctf", "op": "append" });

    let mut sent = Vec::new();
    for _ in 0..6 {
        let url = url.clone();
        let body = body.clone();
        sent.push(tokio::spawn(async move {
            post_json(&url, "/api/chain", &body).await.0
        }));
    }
    for task in sent {
        assert_eq!(task.await.unwrap(), 200);
    }

    let written = std::fs::read_to_string(dir.path().join("chain.gctf")).unwrap();
    assert_eq!(
        written.matches("--- ENDPOINT ---").count(),
        7,
        "one step per edit, and none lost its header: {written}"
    );
    assert_eq!(
        written.matches("--- REQUEST ---").count(),
        7,
        "every step kept its message: {written}"
    );
}

/// The chain endpoint refuses what it cannot express rather than writing it.
#[tokio::test]
async fn a_chain_edit_refuses_the_impossible() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("one.gctf"),
        "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.ok == true\n",
    )
    .unwrap();
    let url = start_server(test_app(dir.path().to_path_buf())).await;

    let (status, _) = post_json(
        &url,
        "/api/chain",
        &serde_json::json!({ "path": "one.gctf", "op": "delete", "index": 0 }),
    )
    .await;
    assert_eq!(status, 400, "a file keeps at least one step");

    let (status, _) = post_json(
        &url,
        "/api/chain",
        &serde_json::json!({ "path": "one.gctf", "op": "shuffle" }),
    )
    .await;
    assert_eq!(status, 400, "an unknown operation is refused");

    let untouched = std::fs::read_to_string(dir.path().join("one.gctf")).unwrap();
    assert!(
        untouched.contains("a.A/One"),
        "nothing was written: {untouched}"
    );
}

/// The project's own call log takes both families through the same writer: a
/// dashboard, a `--data` rerun or a second session reads one file, and an HTTP
/// call that left no line there would be a call the project never saw.
#[tokio::test]
async fn the_project_history_records_both_families() {
    let echo = start_server(echo_app()).await;
    let dir = setup_project_dir("history-both-families");
    let root = dir.join(".grpctestify");
    std::fs::write(
        root.join("collections").join("h.httf"),
        format!("--- ADDRESS ---\n{echo}\n\n--- ENDPOINT ---\nPOST /echo\n\n--- ASSERTS ---\n@status() == 200\n"),
    )
    .unwrap();

    let url = start_server(test_app_project(dir.clone())).await;
    let (status, _) = post_json(
        &url,
        "/api/call",
        &serde_json::json!({
            "collection_path": "h.httf",
            "endpoint": "POST /echo",
            "address": "",
            "bodies_raw": ["{\"a\": 1}"],
            "session_id": "httftest",
        }),
    )
    .await;
    assert_eq!(status, 200);

    let line = std::fs::read_to_string(root.join("history").join("httftest.jsonl"))
        .expect("the call left a line in the project's history");
    let entry: serde_json::Value =
        serde_json::from_str(line.lines().next().unwrap_or_default()).expect("the line is JSON");
    assert_eq!(entry["endpoint"], "POST /echo", "{entry}");
    /* Where it went travels with it, the way it does for a gRPC call: a line
    that cannot say which target answered cannot be replayed. */
    assert!(
        entry["connection"]["address"]
            .as_str()
            .unwrap_or_default()
            .contains("127.0.0.1"),
        "{entry}",
    );
}

/// Regression: a file's own ADDRESS never passes through the browser, so a
/// `{{NAME}}` in it was the one placeholder Execute could not resolve — it
/// dialled the braces while a run of the same file resolved them from the
/// project's active environment.
#[tokio::test]
async fn execute_resolves_a_variable_in_the_file_s_address() {
    let dir = setup_project_dir("call-address-var");
    let root = dir.join(".grpctestify");
    project::write_dotenv(&root, "example", "TARGET=127.0.0.1:1\n").unwrap();
    std::fs::write(
        root.join("collections").join("addr.httf"),
        "--- ADDRESS ---\nhttp://{{TARGET}}\n\n--- ENDPOINT ---\nGET /data.json\n\n--- ASSERTS ---\n@status() == 200\n",
    )
    .unwrap();

    let url = start_server(test_app_project(dir.clone())).await;
    let (status, body) = post_json(
        &url,
        "/api/call",
        &serde_json::json!({
            "collection_path": "addr.httf",
            "endpoint": "GET /data.json",
            "address": "",
            "bodies": [""]
        }),
    )
    .await;

    assert_eq!(status, 200);
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        !error.contains("{{"),
        "the call dialled the braces themselves: {error}"
    );
    assert!(
        error.contains("127.0.0.1:1"),
        "the call went to what the environment names: {error}"
    );
}

/// The rest of the request, resolved where the call is made: a run reads the
/// project's environment for the path, the headers and the body, and Execute
/// sent whatever the browser could not resolve as the braces themselves.
#[tokio::test]
async fn execute_resolves_variables_in_the_path_and_the_headers() {
    let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo.local_addr().unwrap();
    let seen = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
    let recorder = seen.clone();
    tokio::spawn(async move {
        let app = axum::Router::new().fallback(axum::routing::any(
            move |req: axum::http::Request<axum::body::Body>| {
                let recorder = recorder.clone();
                async move {
                    let line = format!(
                        "{} {}",
                        req.uri(),
                        req.headers()
                            .get("x-who")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                    );
                    *recorder.lock().await = line;
                    "ok"
                }
            },
        ));
        axum::serve(echo, app).await.unwrap();
    });

    let dir = setup_project_dir("call-request-vars");
    let root = dir.join(".grpctestify");
    project::write_dotenv(&root, "example", "WHO=Ada\n").unwrap();
    std::fs::write(
        root.join("collections").join("who.httf"),
        format!("--- ADDRESS ---\nhttp://{echo_addr}\n\n--- ENDPOINT ---\nGET /v1/{{{{WHO}}}}\n"),
    )
    .unwrap();

    let url = start_server(test_app_project(dir.clone())).await;
    let (status, body) = post_json(
        &url,
        "/api/call",
        &serde_json::json!({
            "collection_path": "who.httf",
            "endpoint": "GET /v1/{{WHO}}",
            "address": "",
            "headers": { "x-who": "{{WHO}}" },
            "bodies": [""]
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["success"], true, "{body}");
    assert_eq!(
        seen.lock().await.as_str(),
        "/v1/Ada Ada",
        "the path and the header reached the server resolved"
    );
}

/// A file saved into another folder carries its own relative paths — its
/// schema, its certificates — which are read from the directory of the file
/// that names them. Copied down a level they named what sits beside the file it
/// came from, and the run could not find it.
#[tokio::test]
async fn saving_into_another_folder_respells_what_the_file_names() {
    let dir = setup_project_dir("save-as-respell");
    let root = dir.join(".grpctestify");
    let collections = root.join("collections");
    std::fs::create_dir_all(collections.join("auth")).unwrap();
    std::fs::write(collections.join("demo.proto"), "syntax = \"proto3\";\n").unwrap();
    std::fs::write(
        collections.join("here.gctf"),
        "--- ENDPOINT ---\ndemo.S/M\n\n--- PROTO ---\nfiles: demo.proto\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.a == 1\n",
    )
    .unwrap();

    let url = start_server(test_app_project(dir.clone())).await;
    let (status, _) = post_json(
        &url,
        "/api/save",
        &serde_json::json!({
            "path": "auth/copy.gctf",
            "original_path": "here.gctf",
            "content": std::fs::read_to_string(collections.join("here.gctf")).unwrap(),
        }),
    )
    .await;
    assert_eq!(status, 200);

    let written = std::fs::read_to_string(collections.join("auth/copy.gctf")).unwrap();
    assert!(
        written.contains("files: ../demo.proto"),
        "the copy names the schema from where it now lives: {written}"
    );
}
