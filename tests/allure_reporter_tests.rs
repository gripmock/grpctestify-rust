// Allure reporter integration tests — exercise the real AllureReporter

use grpctestify::report::{AllureReporter, Reporter};
use grpctestify::state::{TestResult, TestStatus};

#[test]
fn test_allure_reporter_passing_test() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let pass_result = TestResult {
        name: "/path/to/test_pass.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 100,
        grpc_duration_ms: Some(80),
        error_message: None,
        execution_time: 1700000000,
    };

    // Allure writes files in on_test_end, not on_suite_end
    reporter.on_test_start("test_pass.gctf");
    reporter.on_test_end("test_pass.gctf", &pass_result);

    // Verify output file exists and is valid JSON
    let entries: Vec<_> = std::fs::read_dir(temp_dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .unwrap_or(false)
        })
        .collect();
    assert!(!entries.is_empty(), "Allure report file should exist");

    for entry in &entries {
        let entry = entry.as_ref().unwrap();
        let content = std::fs::read_to_string(entry.path()).expect("Failed to read Allure file");
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        assert_eq!(json["status"], "passed");
        assert!(json.get("uuid").is_some());
        assert!(json.get("historyId").is_some());
        assert!(json.get("labels").is_some());
        assert_eq!(json["stage"], "finished");
    }
}

#[test]
fn test_allure_reporter_failing_test() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let fail_result = TestResult::fail(
        "/path/to/test_fail.gctf",
        "Response mismatch: expected {\"status\":\"ok\"} but got {\"status\":\"error\"}"
            .to_string(),
        200,
        Some(150),
    );

    reporter.on_test_end("test_fail.gctf", &fail_result);

    // Find and verify the failure report
    let entries: Vec<_> = std::fs::read_dir(temp_dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .unwrap_or(false)
        })
        .collect();
    assert!(!entries.is_empty(), "Allure report file should exist");

    for entry in &entries {
        let entry = entry.as_ref().unwrap();
        let content = std::fs::read_to_string(entry.path()).expect("Failed to read Allure file");
        let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

        if json["status"] == "failed" {
            assert!(
                json["statusDetails"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("mismatch")
            );
        }
    }
}

#[test]
fn test_allure_reporter_mixed_results() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_pass.gctf",
        &TestResult {
            name: "/path/test_pass.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            grpc_duration_ms: Some(5),
            error_message: None,
            execution_time: 1700000000,
        },
    );
    reporter.on_test_end(
        "test_fail.gctf",
        &TestResult::fail("/path/test_fail.gctf", "error".to_string(), 20, Some(15)),
    );
    reporter.on_test_end(
        "test_skip.gctf",
        &TestResult {
            name: "/path/test_skip.gctf".to_string(),
            status: TestStatus::Skip,
            duration_ms: 0,
            grpc_duration_ms: None,
            error_message: Some("Skipped".to_string()),
            execution_time: 1700000001,
        },
    );

    // Verify multiple result files created
    let result_files: Vec<_> = std::fs::read_dir(temp_dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        result_files.len() >= 3,
        "At least 3 Allure report files should exist"
    );
}

#[test]
fn test_allure_reporter_labels_present() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_labels.gctf",
        &TestResult {
            name: "test_labels.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            grpc_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
        },
    );

    // Verify labels in output
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();

            let labels = json["labels"]
                .as_array()
                .expect("labels should be an array");
            let label_names: Vec<&str> = labels.iter().filter_map(|l| l["name"].as_str()).collect();

            assert!(
                label_names.contains(&"language"),
                "Should have language label"
            );
            assert!(
                label_names.contains(&"framework"),
                "Should have framework label"
            );
        }
    }
}

#[test]
fn test_allure_reporter_test_name_from_path() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "/workspace/tests/projects/search/case_tech_search.gctf",
        &TestResult {
            name: "/workspace/tests/projects/search/case_tech_search.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 50,
            grpc_duration_ms: Some(40),
            error_message: None,
            execution_time: 1700000000,
        },
    );

    // Verify name is extracted from path
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();

            assert_eq!(json["name"], "case_tech_search.gctf");
            assert_eq!(
                json["fullName"],
                "/workspace/tests/projects/search/case_tech_search.gctf"
            );
        }
    }
}

#[test]
fn test_allure_reporter_timestamps() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_timestamps.gctf",
        &TestResult {
            name: "test_timestamps.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 100,
            grpc_duration_ms: Some(80),
            error_message: None,
            execution_time: 1700000000,
        },
    );

    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();

            let start = json["start"].as_u64().expect("start should be u64");
            let stop = json["stop"].as_u64().expect("stop should be u64");
            assert!(stop >= start, "stop should be >= start");
        }
    }
}
