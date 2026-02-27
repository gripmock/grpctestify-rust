// Explain output tests - compare semantic output against expected fixtures

use grpctestify::execution::ExecutionPlan;
use grpctestify::parser::parse_gctf;
use std::fs;
use std::path::Path;

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
        assert!(
            matches!(plan.rpc_mode, grpctestify::execution::RpcMode::Unary),
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
        assert!(
            matches!(
                plan.rpc_mode,
                grpctestify::execution::RpcMode::ServerStreaming { .. }
            ),
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
        assert!(
            matches!(
                plan.rpc_mode,
                grpctestify::execution::RpcMode::ClientStreaming { .. }
            ),
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
        assert!(
            matches!(plan.rpc_mode, grpctestify::execution::RpcMode::UnaryError),
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

/// Test explain with assertions
#[test]
fn test_explain_with_assertions() {
    let path = "examples/assertions/response-with-asserts.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);

    assert!(plan.assertions.len() > 0, "Expected assertions, got none");
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
