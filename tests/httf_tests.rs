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
        .route("/boom", delete(|| async { StatusCode::IM_A_TEAPOT }));

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
    assert_eq!(result.grpc_status, Some(418));
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
