#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

#[path = "support/mod.rs"]
mod support;
use support::run_cli;
use support::spawn_health_server;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_lists_services() {
    let address = spawn_health_server().await;
    let output = run_cli(&["reflect", "--address", &address, "--plaintext"]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("grpc.health.v1.Health"),
        "expected the health service listed: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_list_methods_shows_signatures() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--list-methods",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("grpc.health.v1.Health/Check"),
        "expected Check method listed: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_describe_shows_request_response_types() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--describe",
        "grpc.health.v1.Health/Check",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("HealthCheckRequest") && stdout.contains("HealthCheckResponse"),
        "expected request/response types described: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_filter_matches_by_service_name() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--filter",
        "health",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("grpc.health.v1.Health") && stdout.contains("Check"),
        "a case-insensitive substring match on the service name must still show its methods: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_filter_matches_by_method_name() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--list-methods",
        "--filter",
        "check",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("grpc.health.v1.Health/Check"),
        "filtering by a method name (not the service name) must still find it: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_filter_excludes_non_matching() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--filter",
        "totally-unrelated-name",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("grpc.health.v1.Health"),
        "a non-matching filter must exclude the service entirely: {stdout}"
    );
    assert!(
        stdout.contains("No services/methods match the given filter"),
        "expected an explicit empty-filter message: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_json_format_is_valid_json() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--format",
        "json",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let services: Vec<String> =
        serde_json::from_slice(&output.stdout).expect("valid JSON service list");
    assert!(services.iter().any(|s| s == "grpc.health.v1.Health"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_describe_text_shows_nested_fields_and_enum_values() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--describe",
        "grpc.health.v1.Health/Check",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("service") && stdout.contains("string"),
        "expected the request's `service: string` field: {stdout}"
    );
    assert!(
        stdout.contains("status") && stdout.contains("ServingStatus"),
        "expected the response's `status: ServingStatus` field: {stdout}"
    );
    assert!(
        stdout.contains("SERVING") && stdout.contains("NOT_SERVING"),
        "expected the enum's possible values listed: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reflect_describe_json_emits_a_json_schema() {
    let address = spawn_health_server().await;
    let output = run_cli(&[
        "reflect",
        "--address",
        &address,
        "--plaintext",
        "--describe",
        "grpc.health.v1.Health/Check",
        "--format",
        "json",
    ]);
    assert!(output.status.success(), "{:?}", output);
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON output");

    assert_eq!(json["input_schema"]["type"], "object");
    assert_eq!(
        json["input_schema"]["properties"]["service"]["type"],
        "string"
    );

    assert_eq!(json["output_schema"]["type"], "object");
    let status_enum = json["output_schema"]["properties"]["status"]["enum"]
        .as_array()
        .expect("status field must carry an enum schema");
    let values: Vec<&str> = status_enum.iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"SERVING"));
    assert!(values.contains(&"NOT_SERVING"));
}
