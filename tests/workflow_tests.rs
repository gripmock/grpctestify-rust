// Workflow tests - test gctf semantics using Workflow built from ExecutionPlan

use grpctestify::execution::{ExecutionPlan, StreamingPattern, Workflow};
use grpctestify::parser::parse_gctf;
use std::path::Path;

/// Test workflow generation for basic unary calls
#[test]
fn test_workflow_from_unary_plan() {
    let test_cases = vec![
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
        let workflow = Workflow::from_plan(&plan);

        // Verify workflow structure
        assert!(workflow.events.len() >= 5, "Too few events for {}", path);

        // Validate workflow
        let result = workflow.validate();
        assert!(
            result.passed,
            "Validation failed for {}: {:?}",
            path, result.errors
        );

        // Verify events
        assert!(
            !workflow.requests().is_empty(),
            "No request events for {}",
            path
        );
        assert!(
            !workflow.responses().is_empty(),
            "No response events for {}",
            path
        );
    }
}

/// Test workflow for streaming
#[test]
fn test_workflow_from_streaming_plan() {
    let test_cases = vec![
        ("examples/streaming/server-streaming.gctf", "Server"),
        ("examples/streaming/client-streaming.gctf", "Client"),
    ];

    for (path, expected_mode) in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        let workflow = Workflow::from_plan(&plan);

        // Check streaming analysis
        let pattern = workflow.analyze_streaming();
        match expected_mode {
            "Server" => assert!(matches!(pattern, StreamingPattern::ServerStreaming { .. })),
            "Client" => assert!(matches!(pattern, StreamingPattern::ClientStreaming { .. })),
            _ => panic!("Unknown mode: {}", expected_mode),
        }
    }
}

/// Test workflow for error cases
#[test]
fn test_workflow_from_error_plan() {
    let test_cases = vec!["examples/error-handling/expected-error.gctf"];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        let workflow = Workflow::from_plan(&plan);

        // Check for Error event
        let has_error = workflow
            .events
            .iter()
            .any(|e| matches!(e, grpctestify::execution::WorkflowEvent::Error { .. }));
        assert!(has_error, "Expected Error event for {}", path);
    }
}

/// Test workflow validation
#[test]
fn test_workflow_validate() {
    let test_cases = vec![
        "examples/basic/unary.gctf",
        "examples/basic/with-headers.gctf",
        "examples/assertions/response-with-asserts.gctf",
    ];

    for path in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path));
        assert!(doc.is_ok(), "Failed to parse {}", path);

        let plan = ExecutionPlan::from_document(&doc.unwrap());
        let workflow = Workflow::from_plan(&plan);
        let result = workflow.validate();

        assert!(
            result.passed,
            "Validation failed for {}: {:?}",
            path, result.errors
        );
    }
}

/// Test workflow with extractions
#[test]
fn test_workflow_with_extractions() {
    let path = "examples/variables/extract-basic.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Verify extraction events
    let extractions = workflow.extractions();
    assert!(!extractions.is_empty(), "Expected extraction events");
}

/// Test workflow with assertions
#[test]
fn test_workflow_with_assertions() {
    let path = "examples/assertions/response-with-asserts.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Verify assertion events
    let assertions = workflow.assertions();
    assert!(!assertions.is_empty(), "Expected assertion events");
}

/// Test workflow event sequence
#[test]
fn test_workflow_event_sequence() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Verify event sequence
    let first = workflow.events.first();
    let last = workflow.events.last();

    assert!(
        matches!(
            first,
            Some(grpctestify::execution::WorkflowEvent::TestLoaded { .. })
        ),
        "Workflow should start with TestLoaded"
    );
    assert!(
        matches!(
            last,
            Some(grpctestify::execution::WorkflowEvent::Complete { .. })
        ),
        "Workflow should end with Complete"
    );
}

/// Test workflow summary accuracy
#[test]
fn test_workflow_summary_accuracy() {
    let test_cases = vec![
        ("examples/basic/unary.gctf", 1, 1),
        ("examples/basic/with-headers.gctf", 1, 1),
    ];

    for (path, expected_requests, expected_responses) in test_cases {
        if !Path::new(path).exists() {
            continue;
        }

        let doc = parse_gctf(Path::new(path)).unwrap();
        let plan = ExecutionPlan::from_document(&doc);
        let workflow = Workflow::from_plan(&plan);

        assert_eq!(
            workflow.summary.total_requests, expected_requests,
            "Total requests mismatch for {}",
            path
        );
        assert_eq!(
            workflow.summary.total_responses, expected_responses,
            "Total responses mismatch for {}",
            path
        );
    }
}

/// Test workflow inline options
#[test]
fn test_workflow_inline_options() {
    // Test partial option
    let doc = parse_gctf(Path::new("examples/basic/partial-match.gctf")).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    let responses = workflow.responses();
    assert!(!responses.is_empty());
    if let Some(grpctestify::execution::WorkflowEvent::ResponseReceived { options, .. }) =
        responses.first()
    {
        assert!(options.partial, "Expected partial=true option");
    }

    // Test tolerance option
    let doc = parse_gctf(Path::new("examples/basic/tolerance.gctf")).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    let responses = workflow.responses();
    assert!(!responses.is_empty());
    if let Some(grpctestify::execution::WorkflowEvent::ResponseReceived { options, .. }) =
        responses.first()
    {
        assert!(options.has_tolerance, "Expected tolerance option");
    }
}

/// Test workflow with headers
#[test]
fn test_workflow_with_headers() {
    let path = "examples/basic/with-headers.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Verify connect event has address
    let has_connect = workflow
        .events
        .iter()
        .any(|e| matches!(e, grpctestify::execution::WorkflowEvent::Connect { .. }));
    assert!(has_connect, "Expected connect event");
}

/// Test multiple requests workflow
#[test]
fn test_workflow_multiple_requests() {
    let path = "examples/streaming/client-streaming.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Count request events
    let requests = workflow.requests();
    assert!(
        requests.len() >= 2,
        "Expected multiple request events, got {}",
        requests.len()
    );
}

/// Test multiple responses workflow
#[test]
fn test_workflow_multiple_responses() {
    let path = "examples/streaming/server-streaming.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Count response events
    let responses = workflow.responses();
    assert!(
        responses.len() >= 3,
        "Expected multiple response events, got {}",
        responses.len()
    );
}

/// Test workflow events by type filtering
#[test]
fn test_workflow_events_by_type() {
    let path = "examples/basic/unary.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Test events_by_type filtering
    let connect_events = workflow.events_by_type("Connect");
    let response_events = workflow.events_by_type("ResponseReceived");

    // Should have at least one connect and one response
    assert!(!connect_events.is_empty(), "Expected Connect events");
    assert!(
        !response_events.is_empty(),
        "Expected ResponseReceived events"
    );

    // Verify all returned events match the requested type
    for event in connect_events {
        assert!(
            matches!(event, grpctestify::execution::WorkflowEvent::Connect { .. }),
            "Expected Connect event, got {:?}",
            event
        );
    }
}

/// Test workflow extract and assert events
#[test]
fn test_workflow_extract_and_assert_events() {
    let path = "examples/variables/extract-with-asserts.gctf";

    if !Path::new(path).exists() {
        return;
    }

    let doc = parse_gctf(Path::new(path)).unwrap();
    let plan = ExecutionPlan::from_document(&doc);
    let workflow = Workflow::from_plan(&plan);

    // Count extract and assert events
    let extracts = workflow.extractions();
    let asserts = workflow.assertions();

    // Should have at least one extract and one assert
    assert!(
        !extracts.is_empty(),
        "Expected Extract events, got {}",
        extracts.len()
    );
    assert!(
        !asserts.is_empty(),
        "Expected Assert events, got {}",
        asserts.len()
    );
}

/// Test workflow validation with invalid workflow
#[test]
fn test_workflow_validate_invalid() {
    use grpctestify::execution::workflow_events::{Workflow, WorkflowEvent, WorkflowSummary};

    // Create a workflow with missing required events
    let workflow = Workflow {
        file_path: "test.gctf".to_string(),
        events: vec![
            WorkflowEvent::TestLoaded {
                file_path: "test.gctf".to_string(),
            },
            // Missing Connect event - should fail validation
            WorkflowEvent::SendRequest {
                backend: "test".to_string(),
                request_index: 0,
                content_type: "application/json".to_string(),
                line_range: (5, 10),
            },
        ],
        summary: WorkflowSummary {
            total_requests: 1,
            total_responses: 0,
            total_extractions: 0,
            total_assertions: 0,
            backends: vec!["test".to_string()],
            rpc_mode: "Unary".to_string(),
            has_streaming: false,
            has_bidi_streaming: false,
        },
    };

    let result = workflow.validate();
    assert!(
        !result.passed,
        "Expected validation to fail for workflow missing Connect event"
    );
    assert!(
        result.errors.iter().any(|e| e.contains("Connect")),
        "Expected error about missing Connect event, got: {:?}",
        result.errors
    );
}
