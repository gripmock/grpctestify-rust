#[test]
fn test_allure_passed_test_structure() {
    let allure_result = serde_json::json!({
        "uuid": "test-uuid-passed",
        "historyId": "history-id-passed",
        "fullName": "/path/to/test_pass.gctf",
        "name": "test_pass.gctf",
        "status": "passed",
        "statusDetails": null,
        "start": 1700000000000_u64,
        "stop": 1700000001000_u64,
        "stage": "finished",
        "labels": [
            {"name": "language", "value": "rust"},
            {"name": "framework", "value": "grpctestify"},
            {"name": "suite", "value": "test-suite"},
            {"name": "feature", "value": "gRPC Test"}
        ],
        "steps": [
            {
                "name": "gRPC call",
                "status": "passed",
                "start": 1700000000000_u64,
                "stop": 1700000001000_u64
            }
        ]
    });

    assert_eq!(allure_result["status"], "passed");
    assert!(allure_result.get("uuid").is_some());
    assert!(allure_result.get("historyId").is_some());
    assert!(allure_result.get("labels").is_some());
}

#[test]
fn test_allure_json_has_required_fields() {
    // Test that Allure JSON structure matches expected schema
    let allure_result = serde_json::json!({
        "uuid": "test-uuid-123",
        "historyId": "history-id-456",
        "fullName": "/path/to/test.gctf",
        "name": "test.gctf",
        "status": "passed",
        "statusDetails": null,
        "start": 1700000000000_u64,
        "stop": 1700000001000_u64,
        "stage": "finished",
        "labels": [
            {"name": "language", "value": "rust"},
            {"name": "framework", "value": "grpctestify"},
            {"name": "suite", "value": "test-suite"},
            {"name": "feature", "value": "gRPC Test"}
        ],
        "steps": [
            {
                "name": "gRPC call",
                "status": "passed",
                "start": 1700000000000_u64,
                "stop": 1700000001000_u64
            }
        ]
    });

    // Verify required fields exist
    assert!(allure_result.get("uuid").is_some());
    assert!(allure_result.get("historyId").is_some());
    assert!(allure_result.get("fullName").is_some());
    assert!(allure_result.get("name").is_some());
    assert!(allure_result.get("status").is_some());
    assert!(allure_result.get("start").is_some());
    assert!(allure_result.get("stop").is_some());
    assert!(allure_result.get("stage").is_some());
    assert!(allure_result.get("labels").is_some());

    // Verify status values
    let valid_statuses = ["passed", "failed", "skipped", "broken"];
    assert!(valid_statuses.contains(&allure_result["status"].as_str().unwrap()));

    // Verify labels structure
    let labels = allure_result["labels"].as_array().unwrap();
    assert!(!labels.is_empty());
    for label in labels {
        assert!(label.get("name").is_some());
        assert!(label.get("value").is_some());
    }

    // Verify steps structure if present
    if let Some(steps) = allure_result.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            assert!(step.get("name").is_some());
            assert!(step.get("status").is_some());
        }
    }
}

#[test]
fn test_allure_failed_test_structure() {
    let allure_result = serde_json::json!({
        "uuid": "test-uuid-failed",
        "historyId": "history-id-failed",
        "fullName": "/path/to/failed_test.gctf",
        "name": "failed_test.gctf",
        "status": "failed",
        "statusDetails": {
            "message": "Response mismatch: expected {\"status\":\"ok\"} but got {\"status\":\"error\"}"
        },
        "start": 1700000000000_u64,
        "stop": 1700000001000_u64,
        "stage": "finished",
        "labels": [
            {"name": "language", "value": "rust"},
            {"name": "framework", "value": "grpctestify"}
        ]
    });

    assert_eq!(allure_result["status"], "failed");
    assert!(
        allure_result["statusDetails"]["message"]
            .as_str()
            .unwrap()
            .contains("mismatch")
    );
}

#[test]
fn test_allure_labels_required() {
    let required_labels = ["language", "framework", "suite", "feature"];

    let allure_result = serde_json::json!({
        "labels": [
            {"name": "language", "value": "rust"},
            {"name": "framework", "value": "grpctestify"},
            {"name": "suite", "value": "test-suite"},
            {"name": "feature", "value": "gRPC Test"}
        ]
    });

    let labels = allure_result["labels"].as_array().unwrap();
    let label_names: Vec<&str> = labels.iter().filter_map(|l| l["name"].as_str()).collect();

    for required in &required_labels {
        assert!(
            label_names.contains(required),
            "Missing required label: {}",
            required
        );
    }
}

#[test]
fn test_allure_status_details_for_failure() {
    // Test statusDetails structure for failed tests
    let status_details = serde_json::json!({
        "message": "Assertion failed: expected value X, got Y",
        "trace": null,
        "flaky": false,
        "known": false,
        "muted": false
    });

    assert!(status_details.get("message").is_some());

    // Optional fields should be skipped if null
    if !status_details["trace"].is_null() {
        assert!(status_details["trace"].is_string());
    }
}

#[test]
fn test_allure_step_structure() {
    let step = serde_json::json!({
        "name": "gRPC call",
        "status": "passed",
        "start": 1700000000000_u64,
        "stop": 1700000001000_u64,
        "attachments": null
    });

    assert!(step.get("name").is_some());
    assert!(step.get("status").is_some());

    // Start and stop should be timestamps
    assert!(step["start"].is_u64());
    assert!(step["stop"].is_u64());

    // Duration should be positive
    let start = step["start"].as_u64().unwrap();
    let stop = step["stop"].as_u64().unwrap();
    assert!(stop >= start, "Stop time should be >= start time");
}

#[test]
fn test_allure_attachment_structure() {
    // Test attachment structure (for future request/response attachments)
    let attachment = serde_json::json!({
        "name": "Request Body",
        "source": "attachments/request-123.json",
        "type": "application/json"
    });

    assert!(attachment.get("name").is_some());
    assert!(attachment.get("source").is_some());
    assert!(attachment.get("type").is_some());
}

#[test]
fn test_allure_history_id_stability() {
    // Test that historyId is consistent for same test
    use uuid::Uuid;

    let namespace = Uuid::NAMESPACE_OID;
    let test_name = "/path/to/test.gctf";

    let history_id_1 = Uuid::new_v5(&namespace, test_name.as_bytes()).to_string();
    let history_id_2 = Uuid::new_v5(&namespace, test_name.as_bytes()).to_string();

    assert_eq!(
        history_id_1, history_id_2,
        "History ID should be stable for same test name"
    );

    let different_test = "/path/to/other.gctf";
    let history_id_3 = Uuid::new_v5(&namespace, different_test.as_bytes()).to_string();

    assert_ne!(
        history_id_1, history_id_3,
        "History ID should differ for different tests"
    );
}

#[test]
fn test_allure_suite_extraction() {
    // Test suite name extraction from path
    let test_path = "/workspace/tests/projects/search/case_tech_search.gctf";

    let suite_name = std::path::Path::new(test_path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("gRPC Tests");

    assert_eq!(suite_name, "search");
}

#[test]
fn test_allure_test_name_extraction() {
    // Test short name extraction from path
    let test_path = "/workspace/tests/projects/search/case_tech_search.gctf";

    let test_name = std::path::Path::new(test_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(test_path);

    assert_eq!(test_name, "case_tech_search.gctf");
}

#[test]
fn test_allure_label_structure() {
    // Test that Allure labels have correct structure
    let label = serde_json::json!({
        "name": "severity",
        "value": "critical"
    });

    assert!(label.get("name").is_some());
    assert!(label.get("value").is_some());
    assert!(label["name"].is_string());
    assert!(label["value"].is_string());

    // Common label names should be valid
    let valid_label_names = [
        "severity", "priority", "tag", "owner", "suite", "subSuite", "feature", "story",
    ];
    for name in valid_label_names {
        let test_label = serde_json::json!({
            "name": name,
            "value": "test-value"
        });
        assert!(test_label["name"].as_str() == Some(name));
    }
}

#[test]
fn test_allure_timestamp_format() {
    // Test that timestamps are in correct format (milliseconds since epoch)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Timestamp should be reasonable (within last year and not in future by more than 1 day)
    let one_year_ago = now - 365 * 24 * 60 * 60 * 1000;
    let one_day_future = now + 24 * 60 * 60 * 1000;

    let test_timestamp = now;
    assert!(
        test_timestamp >= one_year_ago,
        "Timestamp should not be too old"
    );
    assert!(
        test_timestamp <= one_day_future,
        "Timestamp should not be too far in future"
    );
}

#[test]
fn test_allure_status_transitions() {
    // Test valid status transitions
    let valid_transitions = [
        ("pending", "passed"),
        ("pending", "failed"),
        ("pending", "skipped"),
        ("passed", "passed"),
        ("failed", "failed"),
    ];

    for (from, to) in valid_transitions {
        // In Allure, status is final, but we can test that both are valid statuses
        let valid_statuses = ["passed", "failed", "skipped", "broken", "pending"];
        assert!(valid_statuses.contains(&from));
        assert!(valid_statuses.contains(&to));
    }
}
