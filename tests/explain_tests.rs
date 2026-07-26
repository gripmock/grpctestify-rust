#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
// Explain output tests - compare semantic output against expected fixtures

use grpctestify::execution::ExecutionPlan;
use grpctestify::parser::{parse_gctf, parse_gctf_from_str};
use std::assert_matches;
use std::fs;
use std::path::Path;

#[path = "support/mod.rs"]
mod support;

/// A multi-document chain's text output leads with a condensed FLOW summary
/// (step, endpoint, expectation kind) before the full per-document dump —
/// added so a long chain has a table-of-contents rather than requiring a
/// full scroll to see the shape of the scenario.
#[test]
fn test_explain_multi_document_flow_summary() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("chain.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Watch\n\n--- REQUEST ---\n{}\n\n--- ERROR ---\n{\"code\": 5}\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["explain", &file.to_string_lossy()])
        .output()
        .expect("failed to run explain");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let flow_idx = stdout.find("FLOW").expect("FLOW summary header missing");
    let scenario_idx = stdout
        .find("SCENARIO 1")
        .expect("SCENARIO 1 detail missing");
    assert!(
        flow_idx < scenario_idx,
        "FLOW summary must come before the detailed scenarios: {stdout}"
    );
    assert!(stdout.contains("1. grpc.health.v1.Health/Check [response]"));
    assert!(stdout.contains("2. grpc.health.v1.Health/Watch [error]"));
}

/// A multi-document chain's text output also includes a fenced ```mermaid
/// sequenceDiagram``` block — plain text, renders natively when the output
/// is pasted into a markdown file (GitHub/VitePress).
#[test]
fn test_explain_multi_document_mermaid_block() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("chain.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Watch\n\n--- REQUEST ---\n{}\n\n--- ERROR ---\n{\"code\": 5}\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["explain", &file.to_string_lossy()])
        .output()
        .expect("failed to run explain");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("```mermaid"));
    assert!(stdout.contains("sequenceDiagram"));
    assert!(stdout.contains("Client->>Server: 1. grpc.health.v1.Health/Check"));
    assert!(stdout.contains("Server-->>Client: response"));
    assert!(stdout.contains("Client->>Server: 2. grpc.health.v1.Health/Watch"));
    assert!(stdout.contains("Server--xClient: error"));
    assert!(stdout.contains("```\n"));
}

/// Test that explain output matches expected RPC mode
#[test]
fn test_explain_rpc_mode_unary() {
    let test_cases = vec![
        "tests/gctf/basic/unary.gctf",
        "examples/basic/unary.gctf",
        "examples/basic/partial-match.gctf",
        "examples/basic/tolerance.gctf",
    ];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        assert_matches!(
            plan.rpc_mode,
            grpctestify::execution::RpcMode::Unary,
            "Expected Unary mode for {}, got {:?}",
            path,
            plan.rpc_mode
        );
    }
}

#[test]
fn test_explain_rpc_mode_server_streaming() {
    let test_cases = vec![
        "tests/gctf/streaming/server-streaming.gctf",
        "examples/streaming/server-streaming.gctf",
    ];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        assert_matches!(
            plan.rpc_mode,
            grpctestify::execution::RpcMode::ServerStreaming { .. },
            "Expected ServerStreaming mode for {}, got {:?}",
            path,
            plan.rpc_mode
        );
    }
}

#[test]
fn test_explain_rpc_mode_client_streaming() {
    let test_cases = vec![
        "tests/gctf/streaming/client-streaming.gctf",
        "examples/streaming/client-streaming.gctf",
    ];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        assert_matches!(
            plan.rpc_mode,
            grpctestify::execution::RpcMode::ClientStreaming { .. },
            "Expected ClientStreaming mode for {}, got {:?}",
            path,
            plan.rpc_mode
        );
    }
}

#[test]
fn test_explain_rpc_mode_unary_error() {
    let test_cases = vec![
        "tests/gctf/error-handling/expected-error.gctf",
        "examples/error-handling/expected-error.gctf",
    ];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        assert_matches!(
            plan.rpc_mode,
            grpctestify::execution::RpcMode::UnaryError,
            "Expected UnaryError mode for {}, got {:?}",
            path,
            plan.rpc_mode
        );
    }
}

/// Test explain output against expected JSON fixtures
#[test]
fn test_explain_against_fixture() {
    let fixture_path = "tests/fixtures/explain/basic-unary.json";
    let gctf_path = "tests/gctf/basic/unary.gctf";

    if !Path::new(fixture_path).exists() || !Path::new(gctf_path).exists() {
        // Skip if fixtures don't exist yet
        return;
    }

    // Parse the .gctf file
    let doc = parse_gctf(Path::new(gctf_path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    // Load expected fixture
    let expected_json = fs::read_to_string(fixture_path).unwrap();
    let expected: serde_json::Value = serde_json::from_str(&expected_json).unwrap();

    // Convert plan to JSON for comparison
    let actual_json = serde_json::to_value(&plan).unwrap();

    // Compare key fields only (rpc_mode)
    assert_eq!(
        actual_json["rpc_mode"], expected["rpc_mode"],
        "RPC mode mismatch"
    );

    // Compare summary fields
    assert_eq!(
        actual_json["summary"]["total_requests"], expected["summary"]["total_requests"],
        "Total requests mismatch"
    );
    assert_eq!(
        actual_json["summary"]["total_responses"], expected["summary"]["total_responses"],
        "Total responses mismatch"
    );
}

/// Test explain with multiple requests (streaming detection)
#[test]
fn test_explain_multiple_requests() {
    let test_cases = vec![
        (
            "examples/streaming/client-streaming.gctf",
            "ClientStreaming",
        ),
        (
            "examples/streaming/server-streaming.gctf",
            "ServerStreaming",
        ),
    ];

    for (path, expected_mode) in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        let mode_str = format!("{:?}", plan.rpc_mode);
        assert!(
            mode_str.contains(expected_mode),
            "Expected mode '{}' in '{}', got '{}'",
            expected_mode,
            path,
            mode_str
        );
    }
}

/// Test explain with extract sections
#[test]
fn test_explain_with_extractions() {
    let path = "examples/variables/extract-basic.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = match parse_gctf(Path::new(path)) {
        Ok(d) => d,
        Err(_) => return,
    };
    let plan = ExecutionPlan::from_document(&doc);

    // Just verify extractions exist
    assert!(!plan.extractions.is_empty(), "Extractions should exist");
}

/// Single-document `explain` text output must list EXTRACT variables in a
/// deterministic (alphabetical) order — the underlying `SectionContent::Extract`
/// is a `HashMap`, whose iteration order is randomized per-instance, so
/// printing it unsorted made the "↓ Extract:" listing's order flicker
/// between runs. Two other listing sites in the same file already sorted;
/// this one didn't.
#[test]
fn test_explain_single_document_extract_listing_is_alphabetically_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("extract.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\nzebra = .a\napple = .b\nmiddle = .c\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["explain", &file.to_string_lossy()])
        .output()
        .expect("failed to run explain");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let apple = stdout.find("apple").expect("apple missing: {stdout}");
    let middle = stdout.find("middle").expect("middle missing: {stdout}");
    let zebra = stdout.find("zebra").expect("zebra missing: {stdout}");
    assert!(
        apple < middle && middle < zebra,
        "EXTRACT listing must be alphabetically sorted: {stdout}"
    );
}

/// Test explain with assertions
#[test]
fn test_explain_with_assertions() {
    let path = "examples/assertions/response-with-asserts.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    assert!(!plan.assertions.is_empty(), "Expected assertions, got none");
}

/// Test explain summary accuracy
#[test]
fn test_explain_summary_accuracy() {
    let test_cases = vec![
        ("examples/basic/unary.gctf", 1, 1, false, 0),
        ("examples/basic/with-headers.gctf", 1, 1, false, 0),
        ("examples/error-handling/expected-error.gctf", 1, 0, true, 0),
    ];

    for (path, expected_requests, expected_responses, expected_error, _expected_asserts) in
        test_cases
    {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = match parse_gctf(Path::new(path)) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let plan = ExecutionPlan::from_document(&doc);

        assert_eq!(
            plan.summary.total_requests, expected_requests,
            "Total requests mismatch for {}",
            path
        );
        assert_eq!(
            plan.summary.total_responses, expected_responses,
            "Total responses mismatch for {}",
            path
        );
        assert_eq!(
            plan.summary.error_expected, expected_error,
            "Error expected mismatch for {}",
            path
        );
    }
}

#[test]
fn test_explain_error_partial_and_with_asserts_via_ast() {
    let content = r#"--- ENDPOINT ---
tasktracker.TaskService/CompleteTask

--- REQUEST ---
{
  "task_id": "task-42"
}

--- ERROR partial=true with_asserts=true ---
{
  "code": 5
}

--- ASSERTS ---
.message contains "Can't find stub"
"#;

    let doc = parse_gctf_from_str(content, "issue-40-error-options.gctf").unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    assert_eq!(plan.summary.total_errors, 1);
    assert!(plan.summary.error_expected);
    assert_eq!(plan.assertions.len(), 1);

    let error = plan
        .expectations
        .iter()
        .find(|e| e.expectation_type == "error")
        .expect("error expectation");
    assert!(error.comparison_options.partial);
    assert!(error.comparison_options.with_asserts);
}
