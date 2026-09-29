#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use grpctestify::execution::runner::{TestExecutionStatus, TestRunner};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Default)]
struct Seen {
    method: String,
    path: String,
    body: Value,
    raw: String,
    authorization: Option<String>,
    content_type: Option<String>,
}

async fn serve() -> (String, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));

    let record = {
        let seen = seen.clone();
        move |method: &str, path: String, headers: axum::http::HeaderMap, body: Value| {
            let mut slot = seen.lock().unwrap_or_else(|e| e.into_inner());
            slot.method = method.to_string();
            slot.path = path;
            slot.body = body;
            slot.authorization = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            slot.content_type = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
        }
    };

    let app = Router::new()
        .route(
            "/v1/users",
            post({
                let record = record.clone();
                move |headers: axum::http::HeaderMap, Json(body): Json<Value>| async move {
                    record("POST", "/v1/users".to_string(), headers, body);
                    (
                        StatusCode::CREATED,
                        Json(json!({"id": "u-1", "name": "Ada"})),
                    )
                }
            }),
        )
        .route(
            "/v1/users/{id}",
            get({
                let record = record.clone();
                move |Path(id): Path<String>, headers: axum::http::HeaderMap| async move {
                    record("GET", format!("/v1/users/{id}"), headers, Value::Null);
                    Json(json!({"id": id, "name": "Ada", "active": true}))
                }
            })
            .delete({
                let record = record.clone();
                move |Path(id): Path<String>, headers: axum::http::HeaderMap| async move {
                    record("DELETE", format!("/v1/users/{id}"), headers, Value::Null);
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            "/raw",
            post({
                let seen = seen.clone();
                move |headers: axum::http::HeaderMap, body: String| async move {
                    let mut slot = seen.lock().unwrap_or_else(|e| e.into_inner());
                    slot.method = "POST".to_string();
                    slot.path = "/raw".to_string();
                    slot.raw = body;
                    slot.content_type = headers
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    StatusCode::OK
                }
            }),
        )
        .route(
            "/plain",
            get(|| async { ([("content-type", "text/plain")], "not json at all") }),
        )
        .route("/boom", delete(|| async { StatusCode::IM_A_TEAPOT }))
        .route(
            "/hop",
            get(|| async { axum::response::Redirect::temporary("/v1/users/8") }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (address, seen)
}

async fn run(content: &str) -> grpctestify::execution::runner::TestExecutionResult {
    let document = grpctestify::parser::parse_gctf_from_str(content, "t.httf").expect("parses");
    TestRunner::new(false, 10, false, false, false, None)
        .with_capture_exchange(true)
        .run_test(&document)
        .await
        .expect("runs")
}

fn reason(result: &grpctestify::execution::runner::TestExecutionResult) -> String {
    match &result.status {
        TestExecutionStatus::Pass => "passed".to_string(),
        TestExecutionStatus::Fail(message) => message.clone(),
    }
}

#[tokio::test]
async fn a_post_sends_its_body_and_reads_the_answer() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST_HEADERS ---\nauthorization: Bearer t0ken\n\n--- REQUEST ---\n{{\"name\": \"Ada\"}}\n\n--- ASSERTS ---\n.name == \"Ada\"\n@status() == 201\n"
    ))
    .await;

    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/v1/users");
    assert_eq!(seen.body, json!({"name": "Ada"}));
    assert_eq!(seen.authorization.as_deref(), Some("Bearer t0ken"));
    assert_eq!(seen.content_type.as_deref(), Some("application/json"));
}

#[tokio::test]
async fn a_response_section_is_compared_against_the_body() {
    let (address, _) = serve().await;
    let ok = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /v1/users/7\n\n--- RESPONSE ---\n{{\"id\": \"7\", \"name\": \"Ada\", \"active\": true}}\n"
    ))
    .await;
    assert!(
        matches!(ok.status, TestExecutionStatus::Pass),
        "{}",
        reason(&ok)
    );

    let bad = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /v1/users/7\n\n--- RESPONSE ---\n{{\"id\": \"7\", \"name\": \"Grace\", \"active\": true}}\n"
    ))
    .await;
    assert!(
        reason(&bad).contains("Response mismatch"),
        "{}",
        reason(&bad)
    );
}

#[tokio::test]
async fn a_status_that_is_not_expected_fails_and_says_which_one_came_back() {
    let (address, _) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nDELETE /boom\n\n--- ASSERTS ---\n@status() == 204\n"
    ))
    .await;
    let message = reason(&result);
    assert!(message.contains("418"), "{message}");
    assert_eq!(result.http_status, Some(418));
    assert_eq!(
        result.grpc_status, None,
        "an HTTP status is not a gRPC code"
    );
}

#[tokio::test]
async fn a_method_with_no_body_sends_none() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nDELETE /v1/users/9\n\n--- ASSERTS ---\n@status() == 204\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.method, "DELETE");
    assert_eq!(seen.content_type, None);
}

#[tokio::test]
async fn a_body_that_is_not_json_arrives_as_a_string() {
    let (address, _) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /plain\n\n--- ASSERTS ---\n. == \"not json at all\"\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
}

#[tokio::test]
async fn a_chain_carries_what_it_extracted_into_the_next_step() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST ---\n{{\"name\": \"Ada\"}}\n\n--- EXTRACT ---\nuser = .id\n\n--- ENDPOINT ---\nGET /v1/users/{{{{user}}}}\n\n--- ASSERTS ---\n.id == \"u-1\"\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.path, "/v1/users/u-1");
}

/// A later step keeps the address the chain started with — reading the
/// environment first sent every step after the first to the environment's
/// address, and only in a project that had one.
#[tokio::test]
async fn a_chain_step_keeps_the_address_over_the_environment() {
    let (address, seen) = serve().await;
    let document = grpctestify::parser::parse_gctf_from_str(
        &format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nPOST /v1/users\n\n--- REQUEST ---\n{{\"name\": \"Ada\"}}\n\n--- EXTRACT ---\nuser = .id\n\n--- ENDPOINT ---\nGET /v1/users/{{{{user}}}}\n\n--- ASSERTS ---\n.id == \"u-1\"\n"
        ),
        "t.httf",
    )
    .expect("parses");
    let result = TestRunner::new(false, 10, false, false, false, None)
        .with_env_address("http://127.0.0.1:1".to_string())
        .run_test(&document)
        .await
        .expect("runs");
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.path, "/v1/users/u-1");
}

#[tokio::test]
async fn a_target_that_does_not_answer_says_so_with_its_url() {
    let result = run("--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /health\n\n--- ASSERTS ---\n@status() == 200\n").await;
    let message = reason(&result);
    assert!(message.contains("127.0.0.1:1"), "{message}");
}

#[tokio::test]
async fn an_absolute_url_ignores_the_address() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET {address}/v1/users/3\n\n--- ASSERTS ---\n.id == \"3\"\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    assert_eq!(
        seen.lock().unwrap_or_else(|e| e.into_inner()).path,
        "/v1/users/3"
    );
}

#[tokio::test]
async fn a_form_body_is_sent_as_written_with_the_type_it_implies() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nPOST /raw\n\n--- REQUEST ---\nname=Ada&age=36\n\n--- ASSERTS ---\n@status() == 200\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.raw, "name=Ada&age=36");
    assert_eq!(
        seen.content_type.as_deref(),
        Some("application/x-www-form-urlencoded")
    );
}

#[tokio::test]
async fn a_declared_content_type_wins_over_the_one_the_body_implies() {
    let (address, seen) = serve().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nPOST /raw\n\n--- REQUEST_HEADERS ---\ncontent-type: text/csv\n\n--- REQUEST ---\nid,name\n1,Ada\n\n--- ASSERTS ---\n@status() == 200\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(seen.raw, "id,name\n1,Ada");
    assert_eq!(seen.content_type.as_deref(), Some("text/csv"));
}

#[tokio::test]
async fn a_response_section_can_be_plain_text() {
    let (address, _) = serve().await;
    let ok = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /plain\n\n--- RESPONSE ---\nnot json at all\n"
    ))
    .await;
    assert!(
        matches!(ok.status, TestExecutionStatus::Pass),
        "{}",
        reason(&ok)
    );

    let bad = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /plain\n\n--- RESPONSE ---\nsomething else\n"
    ))
    .await;
    assert!(
        reason(&bad).contains("Response mismatch"),
        "{}",
        reason(&bad)
    );
}

#[tokio::test]
async fn the_status_is_checked_the_way_gctf_checks_one() {
    let (address, _) = serve().await;
    let ok = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nDELETE /v1/users/4\n\n--- ASSERTS ---\n@status() == 204\n"
    ))
    .await;
    assert!(
        matches!(ok.status, TestExecutionStatus::Pass),
        "{}",
        reason(&ok)
    );

    let bad = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nDELETE /v1/users/4\n\n--- ASSERTS ---\n@status() == 200\n"
    ))
    .await;
    assert!(reason(&bad).contains("204"), "{}", reason(&bad));
}

#[tokio::test]
async fn a_redirect_is_the_answer_unless_options_ask_to_follow_it() {
    let (address, _) = serve().await;
    let stays = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /hop\n\n--- ASSERTS ---\n@status() == 307\n@header(\"location\") == \"/v1/users/8\"\n"
    ))
    .await;
    assert!(
        matches!(stays.status, TestExecutionStatus::Pass),
        "{}",
        reason(&stays)
    );
    assert_eq!(stays.http_status, Some(307));

    let follows = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /hop\n\n--- OPTIONS ---\nfollow_redirects: true\n\n--- ASSERTS ---\n@status() == 200\n.id == \"8\"\n"
    ))
    .await;
    assert!(
        matches!(follows.status, TestExecutionStatus::Pass),
        "{}",
        reason(&follows)
    );
    assert_eq!(follows.http_status, Some(200));
}

#[tokio::test]
async fn request_headers_reach_the_server_in_file_order() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = format!("http://{}", listener.local_addr().expect("addr"));
    let seen = Arc::new(Mutex::new(String::new()));
    let recorder = seen.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = vec![0u8; 4096];
        let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
            .await
            .unwrap_or(0);
        *recorder.lock().unwrap_or_else(|e| e.into_inner()) =
            String::from_utf8_lossy(&buf[..n]).to_string();
        let _ = tokio::io::AsyncWriteExt::write_all(
            &mut socket,
            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
        )
        .await;
    });

    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /order\n\n--- REQUEST_HEADERS ---\nx-zulu: 1\nx-alpha: 2\nx-mike: 3\nx-bravo: 4\n\n--- ASSERTS ---\n@status() == 200\n"
    ))
    .await;
    assert!(
        matches!(result.status, TestExecutionStatus::Pass),
        "{}",
        reason(&result)
    );
    let request = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let positions: Vec<usize> = ["x-zulu", "x-alpha", "x-mike", "x-bravo"]
        .iter()
        .map(|name| {
            request
                .find(name)
                .unwrap_or_else(|| panic!("{name}: {request}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "headers arrive as the file wrote them: {request}"
    );
}

async fn origin_that_never_answers() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = format!("http://{}", listener.local_addr().expect("addr"));
    let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = connections.clone();
    tokio::spawn(async move {
        loop {
            let Ok(socket) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            drop(socket);
        }
    });
    (address, connections)
}

/// A step dials once: the retry budget (`--retry`, `OPTIONS.retry`, `#[retry(N)]`,
/// and `no_retry` over all of them) is resolved and spent by the run loop, so a
/// second loop inside the step would multiply it. `httf_retry_tests.rs` counts
/// the dials the budget actually buys.
#[tokio::test]
async fn a_step_dials_once_because_the_retry_budget_belongs_to_the_run_loop() {
    let (address, connections) = origin_that_never_answers().await;
    let result = run(&format!(
        "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\nGET /health\n\n#[retry(2)]\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n@status() == 200\n"
    ))
    .await;

    assert!(
        matches!(result.status, TestExecutionStatus::Fail(_)),
        "nothing answered"
    );
    assert_eq!(
        result.failure_kind,
        Some(grpctestify::execution::runner::FailureKind::Transport)
    );
    assert!(!result.retried);
    assert_eq!(
        connections.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the step itself dials once"
    );
}

#[test]
fn status_on_a_grpc_step_says_it_is_for_http() {
    let engine = grpctestify::assert::AssertionEngine::with_registry(Arc::new(
        grpctestify::execution::plugin_dir::build_plugin_manager(),
    ));
    let headers: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let outcome = engine
        .evaluate_with_timing(
            "@status() == 200",
            &json!({"status": "SERVING"}),
            Some(&headers),
            None,
            None,
            &std::collections::HashMap::new(),
            Some("grpc"),
        )
        .expect("evaluates");
    let message = format!("{outcome:?}");
    assert!(
        message.contains("Error("),
        "an error, not a false: {message}"
    );
    assert!(message.contains("@status() is for HTTP tests"), "{message}");
    assert!(message.contains("grpc answer"), "{message}");
}
