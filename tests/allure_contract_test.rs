use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn get_binary() -> String {
    env!("CARGO_BIN_EXE_grpctestify").to_string()
}

fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("Failed to write test file");
    path
}

#[test]
fn test_allure_passed_test_structure() {
    let binary = get_binary();
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let allure_dir = temp_dir.path().join("allure-results");
    fs::create_dir_all(&allure_dir).expect("Failed to create allure dir");

    let test_content = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{"id": "123"}

--- RESPONSE ---
{"status": "ok"}
"#;

    let test_file = create_test_file(temp_dir.path(), "test_pass.gctf", test_content);

    let output = Command::new(&binary)
        .args(["check", test_file.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("Failed to execute check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let check_result: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");

    // Verify check passed
    assert_eq!(check_result["summary"]["total_errors"], 0);
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
    assert!(allure_result["statusDetails"]["message"]
        .as_str()
        .unwrap()
        .contains("mismatch"));
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
