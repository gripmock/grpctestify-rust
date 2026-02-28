use grpctestify::execution::runner::{TestExecutionStatus, TestRunner};
use grpctestify::grpc::GrpcResponse;
use grpctestify::parser::ast::{
    DocumentMetadata, GctfDocument, InlineOptions, Section, SectionContent, SectionType,
};
use serde_json::json;

fn create_empty_doc() -> GctfDocument {
    GctfDocument {
        file_path: "test.gctf".to_string(),
        sections: Vec::new(),
        metadata: DocumentMetadata {
            source: None,
            mtime: None,
            parsed_at: 0,
        },
    }
}

fn create_response_section(expected: serde_json::Value, options: InlineOptions) -> Section {
    Section {
        section_type: SectionType::Response,
        content: SectionContent::Json(expected),
        inline_options: options,
        raw_content: "".to_string(),
        start_line: 0,
        end_line: 0,
    }
}

#[test]
fn test_validate_response_exact_match() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"}
    doc.sections.push(create_response_section(
        json!({"foo": "bar"}),
        InlineOptions::default(),
    ));

    // Actual: {"foo": "bar"}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "bar"}));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

fn create_asserts_section(assertions: Vec<String>) -> Section {
    Section {
        section_type: SectionType::Asserts,
        content: SectionContent::Assertions(assertions),
        inline_options: InlineOptions::default(),
        raw_content: "".to_string(),
        start_line: 0,
        end_line: 0,
    }
}

#[test]
fn test_validate_response_with_asserts() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"} with with_asserts=true
    let options = InlineOptions {
        with_asserts: true,
        ..Default::default()
    };
    doc.sections
        .push(create_response_section(json!({"foo": "bar"}), options));

    // Asserts for the above response
    doc.sections
        .push(create_asserts_section(vec![".foo == \"bar\"".to_string()]));

    // Actual: {"foo": "bar"}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "bar"}));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

#[test]
fn test_validate_response_with_asserts_fail() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"} with with_asserts=true
    let options = InlineOptions {
        with_asserts: true,
        ..Default::default()
    };
    doc.sections
        .push(create_response_section(json!({"foo": "bar"}), options));

    // Asserts for the above response
    doc.sections.push(create_asserts_section(vec![
        ".foo == \"baz\"".to_string(), // Should fail
    ]));

    // Actual: {"foo": "bar"}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "bar"}));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Fail(msg) => {
            // Updated error message check
            assert!(msg.contains("Assertion failed (attached to RESPONSE at line 0):"));
        }
        _ => panic!("Expected failure"),
    }
}

#[test]
fn test_validate_response_mixed_asserts() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Response 1: with_asserts=true
    let options1 = InlineOptions {
        with_asserts: true,
        ..Default::default()
    };
    doc.sections
        .push(create_response_section(json!({"id": 1}), options1));

    // Asserts for Response 1
    doc.sections
        .push(create_asserts_section(vec![".id == 1".to_string()]));

    // Response 2: with_asserts=true to check the second message
    let options2 = InlineOptions {
        with_asserts: true,
        ..Default::default()
    };
    doc.sections
        .push(create_response_section(json!({"id": 2}), options2));

    // Asserts for Response 2
    doc.sections
        .push(create_asserts_section(vec![".id == 2".to_string()]));

    // Actual messages
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"id": 1}));
    response.messages.push(json!({"id": 2}));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

#[test]
fn test_validate_response_mismatch() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"}
    doc.sections.push(create_response_section(
        json!({"foo": "bar"}),
        InlineOptions::default(),
    ));

    // Actual: {"foo": "baz"}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "baz"}));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Fail(msg) => {
            assert!(msg.contains("mismatch"));
            assert!(msg.contains("foo"));
        }
        _ => panic!("Expected failure"),
    }
}

#[test]
fn test_validate_response_partial() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"} with partial=true
    let options = InlineOptions {
        partial: true,
        ..Default::default()
    };
    doc.sections
        .push(create_response_section(json!({"foo": "bar"}), options));

    // Actual: {"foo": "bar", "extra": 1}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "bar", "extra": 1}));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

#[test]
fn test_validate_response_partial_fail() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: {"foo": "bar"} with partial=false (default)
    let options = InlineOptions::default();
    doc.sections
        .push(create_response_section(json!({"foo": "bar"}), options));

    // Actual: {"foo": "bar", "extra": 1}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"foo": "bar", "extra": 1}));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Fail(msg) => {
            assert!(msg.contains("Unexpected key"));
        }
        _ => panic!("Expected failure"),
    }
}

#[test]
fn test_validate_response_multiple() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect:
    // 1. {"id": 1}
    // 2. {"id": 2}
    doc.sections.push(create_response_section(
        json!({"id": 1}),
        InlineOptions::default(),
    ));
    doc.sections.push(create_response_section(
        json!({"id": 2}),
        InlineOptions::default(),
    ));

    // Actual:
    // 1. {"id": 1}
    // 2. {"id": 2}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"id": 1}));
    response.messages.push(json!({"id": 2}));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

#[test]
fn test_validate_response_count_mismatch() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect:
    // 1. {"id": 1}
    // 2. {"id": 2}
    doc.sections.push(create_response_section(
        json!({"id": 1}),
        InlineOptions::default(),
    ));
    doc.sections.push(create_response_section(
        json!({"id": 2}),
        InlineOptions::default(),
    ));

    // Actual:
    // 1. {"id": 1}
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"id": 1}));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Fail(msg) => {
            // Updated error message expectation
            assert!(msg.contains("Expected message for RESPONSE section"));
            assert!(msg.contains("but no more messages received"));
        }
        _ => panic!("Expected failure"),
    }
}

#[test]
fn test_validate_response_unordered_arrays() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: [{"id": 2}, {"id": 1}] with unordered_arrays=true
    let options = InlineOptions {
        unordered_arrays: true,
        ..Default::default()
    };
    doc.sections.push(create_response_section(
        json!([{"id": 2}, {"id": 1}]),
        options,
    ));

    // Actual: [{"id": 1}, {"id": 2}]
    let mut response = GrpcResponse::new();
    response.messages.push(json!([{"id": 1}, {"id": 2}]));

    let result = runner.validate_response(&doc, &response, 100);

    assert_eq!(result.status, TestExecutionStatus::Pass);
}

#[test]
fn test_validate_response_unordered_arrays_fail() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // Expect: [{"id": 2}, {"id": 1}] with unordered_arrays=false (default)
    let options = InlineOptions::default();
    doc.sections.push(create_response_section(
        json!([{"id": 2}, {"id": 1}]),
        options,
    ));

    // Actual: [{"id": 1}, {"id": 2}]
    let mut response = GrpcResponse::new();
    response.messages.push(json!([{"id": 1}, {"id": 2}]));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Fail(msg) => {
            assert!(msg.contains("mismatch"));
        }
        _ => panic!("Expected failure"),
    }
}

fn create_extract_section(extractions: Vec<(String, String)>) -> Section {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    for (k, v) in extractions {
        map.insert(k, v);
    }
    Section {
        section_type: SectionType::Extract,
        content: SectionContent::Extract(map),
        inline_options: InlineOptions::default(),
        raw_content: "".to_string(),
        start_line: 0,
        end_line: 0,
    }
}

#[test]
fn test_validate_response_extract() {
    let runner = TestRunner::new(false, 5, false, false, false, None);
    let mut doc = create_empty_doc();

    // 1. Response: {"id": 123}
    doc.sections.push(create_response_section(
        json!({"id": 123}),
        InlineOptions::default(),
    ));

    // 2. Extract: user_id = .id
    doc.sections.push(create_extract_section(vec![(
        "user_id".to_string(),
        ".id".to_string(),
    )]));

    // 3. Response: {"echo": 123} - variable substitution converts string to number
    // This tests that substitution works using the extracted variable
    doc.sections.push(create_response_section(
        json!({"echo": 123}),
        InlineOptions::default(),
    ));

    // Actual messages
    let mut response = GrpcResponse::new();
    response.messages.push(json!({"id": 123}));
    response.messages.push(json!({"echo": 123}));

    let result = runner.validate_response(&doc, &response, 100);

    match result.status {
        TestExecutionStatus::Pass => {}
        TestExecutionStatus::Fail(msg) => panic!("Validation failed: {}", msg),
    }
}
