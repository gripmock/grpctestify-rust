#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
// Allure reporter integration tests — exercise the real AllureReporter

use grpctestify::report::{AllureReporter, Reporter};
use grpctestify::state::{CapturedExchange, ConfigSummary, TestMeta, TestResult, TestStatus};
use std::collections::HashMap;

#[test]
fn allure_reporter_passing_test() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let pass_result = TestResult {
        name: "/path/to/test_pass.gctf".to_string(),
        family: "gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 100,
        call_duration_ms: Some(80),
        error_message: None,
        execution_time: 1700000000,
        meta: TestMeta::default(),
        assertions: Vec::new(),
        exchange: None,
        retried: false,
        document_durations_ms: Vec::new(),
        row_params: Vec::new(),
        config_summary: ConfigSummary::default(),
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
fn allure_reporter_failing_test() {
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
fn allure_reporter_mixed_results() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_pass.gctf",
        &TestResult {
            name: "/path/test_pass.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: Some(5),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
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
            family: "gctf".to_string(),
            status: TestStatus::Skip,
            duration_ms: 0,
            call_duration_ms: None,
            error_message: Some("Skipped".to_string()),
            execution_time: 1700000001,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
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
fn allure_reporter_labels_present() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_labels.gctf",
        &TestResult {
            name: "test_labels.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
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
fn allure_reporter_test_name_from_path() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "/workspace/tests/projects/search/case_tech_search.gctf",
        &TestResult {
            name: "/workspace/tests/projects/search/case_tech_search.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 50,
            call_duration_ms: Some(40),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
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
fn allure_reporter_timestamps() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test_timestamps.gctf",
        &TestResult {
            name: "test_timestamps.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 100,
            call_duration_ms: Some(80),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
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

#[test]
fn allure_reporter_tags_and_owner_labels() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let meta = TestMeta {
        tags: vec!["smoke".to_string(), "fast".to_string()],
        owner: Some("backend-qa".to_string()),
        ..TestMeta::default()
    };

    reporter.on_test_end(
        "test_labels.gctf",
        &TestResult {
            name: "test_labels.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta,
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            let labels = json["labels"].as_array().expect("labels should be array");
            let label_pairs: Vec<(&str, &str)> = labels
                .iter()
                .filter_map(|l| Some((l["name"].as_str()?, l["value"].as_str()?)))
                .collect();
            assert!(label_pairs.contains(&("tag", "smoke")));
            assert!(label_pairs.contains(&("tag", "fast")));
            assert!(label_pairs.contains(&("owner", "backend-qa")));
        }
    }
}

#[test]
fn allure_reporter_grpc_labels_for_filtering() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let test_file = temp_dir.path().join("multi.gctf");
    std::fs::write(
        &test_file,
        r#"--- ENDPOINT ---
tasktracker.TaskService/CompleteTask

--- REQUEST ---
{"task_id":"task-42"}

--- RESPONSE ---
{"ok":true}

--- ENDPOINT ---
tasktracker.TaskService/GetTask

--- REQUEST ---
{"task_id":"task-42"}

--- RESPONSE ---
{"ok":true}
"#,
    )
    .expect("write gctf");

    reporter.on_test_end(
        test_file.to_string_lossy().as_ref(),
        &TestResult {
            name: test_file.to_string_lossy().to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 20,
            call_duration_ms: Some(10),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            let labels = json["labels"].as_array().expect("labels array");

            let has_service = labels
                .iter()
                .any(|l| l["name"] == "grpc_service" && l["value"] == "TaskService");
            let has_method_complete = labels
                .iter()
                .any(|l| l["name"] == "grpc_method" && l["value"] == "CompleteTask");
            let has_method_get = labels
                .iter()
                .any(|l| l["name"] == "grpc_method" && l["value"] == "GetTask");
            let has_endpoint = labels.iter().any(|l| {
                l["name"] == "grpc_endpoint" && l["value"] == "tasktracker.TaskService/CompleteTask"
            });

            if has_service && has_method_complete && has_method_get && has_endpoint {
                found = true;
            }
        }
    }

    assert!(found, "expected grpc_* labels in allure report");
}

#[test]
fn allure_reporter_runtime_parameters_present() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let test_file = temp_dir.path().join("params.gctf");
    std::fs::write(
        &test_file,
        r#"--- OPTIONS ---
timeout: 15
retry: 2
retry-delay: 0.3
no-retry: false
compression: gzip

--- ENDPOINT ---
tasktracker.TaskService/GetTask

--- REQUEST ---
{"task_id":"task-42"}

--- RESPONSE ---
{"ok":true}
"#,
    )
    .expect("write gctf");

    reporter.on_test_end(
        test_file.to_string_lossy().as_ref(),
        &TestResult {
            name: test_file.to_string_lossy().to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: Some(5),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            let params = json["parameters"].as_array().expect("parameters array");

            let has_version = params.iter().any(|p| p["name"] == "grpctestify.version");
            let has_timeout = params
                .iter()
                .any(|p| p["name"] == "runtime.timeout" && p["value"] == "15");
            let has_retry = params
                .iter()
                .any(|p| p["name"] == "runtime.retry" && p["value"] == "2");
            let has_docs = params
                .iter()
                .any(|p| p["name"] == "documents.count" && p["value"] == "1");

            if has_version && has_timeout && has_retry && has_docs {
                found = true;
            }
        }
    }

    assert!(
        found,
        "expected runtime/version parameters in allure report"
    );
}

// Regression: a test expanded from a `--data` row must carry that row's
// values as Allure parameters, so history/retries can be traced back to the
// exact row that produced them.
#[test]
fn allure_reporter_row_params_present() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let test_file = temp_dir.path().join("row.gctf#[row=0 user=alice]");
    reporter.on_test_end(
        test_file.to_string_lossy().as_ref(),
        &TestResult {
            name: test_file.to_string_lossy().to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: Some(5),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: vec![
                ("id".to_string(), "1".to_string()),
                ("user".to_string(), "alice".to_string()),
            ],
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            let params = json["parameters"].as_array().expect("parameters array");
            let has_user = params
                .iter()
                .any(|p| p["name"] == "user" && p["value"] == "alice");
            let has_id = params
                .iter()
                .any(|p| p["name"] == "id" && p["value"] == "1");
            if has_user && has_id {
                found = true;
            }
        }
    }
    assert!(found, "expected row values as Allure parameters");
}

#[test]
fn allure_reporter_display_name_from_meta() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let meta = TestMeta {
        name: Some("Custom Test Name".to_string()),
        ..TestMeta::default()
    };

    reporter.on_test_end(
        "test.gctf",
        &TestResult {
            name: "test.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta,
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(json["name"].as_str(), Some("Custom Test Name"));
        }
    }
}

#[test]
fn allure_reporter_description_and_links_from_meta() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let meta = TestMeta {
        summary: Some("Verifies the checkout flow returns a receipt".to_string()),
        links: vec!["https://example.com/JIRA-123".to_string()],
        ..TestMeta::default()
    };

    reporter.on_test_end(
        "test.gctf",
        &TestResult {
            name: "test.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta,
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(
                json["description"].as_str(),
                Some("Verifies the checkout flow returns a receipt")
            );
            assert_eq!(
                json["links"][0]["url"].as_str(),
                Some("https://example.com/JIRA-123")
            );
            assert_eq!(json["links"][0]["type"].as_str(), Some("custom"));
            found = true;
        }
    }
    assert!(found, "expected a result json file to be written");
}

#[test]
fn allure_reporter_no_description_or_links_when_meta_empty() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test.gctf",
        &TestResult {
            name: "test.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(json.get("description").is_none());
            assert!(json.get("links").is_none());
            found = true;
        }
    }
    assert!(found, "expected a result json file to be written");
}

#[test]
fn allure_reporter_writes_exchange_attachment() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/grpc".to_string());
    let exchange = CapturedExchange::capture(
        headers,
        HashMap::new(),
        vec![serde_json::json!({"message": "Hello, World!"})],
    );

    reporter.on_test_end(
        "test.gctf",
        &TestResult {
            name: "test.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: Some(5),
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: Some(exchange),
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut result_json = None;
    let mut attachment_files = Vec::new();
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with("-result.json") {
            result_json = Some(
                serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap())
                    .unwrap(),
            );
        } else if path
            .to_string_lossy()
            .ends_with("-exchange-attachment.json")
        {
            attachment_files.push(path);
        }
    }

    let result_json = result_json.expect("expected a -result.json file");
    let attachments = result_json["attachments"]
        .as_array()
        .expect("expected attachments array on the result");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["type"], "application/json");
    let source = attachments[0]["source"].as_str().unwrap();

    assert_eq!(
        attachment_files.len(),
        1,
        "expected one attachment file written to disk"
    );
    assert_eq!(
        attachment_files[0].file_name().unwrap().to_str().unwrap(),
        source,
        "result.json must reference the attachment file that was actually written"
    );

    let attachment_content: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&attachment_files[0]).unwrap()).unwrap();
    assert_eq!(
        attachment_content["headers"]["content-type"],
        "application/grpc"
    );
    assert_eq!(
        attachment_content["response"][0]["message"],
        "Hello, World!"
    );
    assert_eq!(attachment_content["truncated"], false);
}

#[test]
fn allure_reporter_no_attachment_when_exchange_absent() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    reporter.on_test_end(
        "test.gctf",
        &TestResult {
            name: "test.gctf".to_string(),
            family: "gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 10,
            call_duration_ms: None,
            error_message: None,
            execution_time: 1700000000,
            meta: TestMeta::default(),
            assertions: Vec::new(),
            exchange: None,
            retried: false,
            document_durations_ms: Vec::new(),
            row_params: Vec::new(),
            config_summary: ConfigSummary::default(),
        },
    );

    let mut found = false;
    for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            !path
                .to_string_lossy()
                .ends_with("-exchange-attachment.json"),
            "no attachment file should be written when exchange is None"
        );
        if path.to_string_lossy().ends_with("-result.json") {
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(json.get("attachments").is_none());
            found = true;
        }
    }
    assert!(found, "expected a result json file to be written");
}

/// The step this report shows for a call is named after the call the file
/// made: a suite with `.httf` files in it named every one of them `gRPC call`.
#[test]
fn an_http_test_reports_an_http_call_step() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let reporter = AllureReporter::new(temp_dir.path().to_path_buf());

    let result = TestResult {
        name: "/path/to/probe.httf".to_string(),
        family: "httf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 12,
        call_duration_ms: Some(9),
        error_message: None,
        execution_time: 1700000000,
        meta: TestMeta::default(),
        assertions: Vec::new(),
        exchange: None,
        retried: false,
        document_durations_ms: Vec::new(),
        row_params: Vec::new(),
        config_summary: ConfigSummary::default(),
    };

    reporter.on_test_start("probe.httf");
    reporter.on_test_end("probe.httf", &result);

    let written = std::fs::read_dir(temp_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .expect("Allure report file should exist");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(written.path()).unwrap()).unwrap();
    let steps = json["steps"].as_array().expect("the call is a step");
    assert_eq!(steps[0]["name"], "HTTP call", "{json}");
}

/// And where the report walks the file's own documents, the label beside each
/// one is the shape that document has: `Unary` is gRPC's word, and an HTTP
/// request has no RPC mode to name.
#[test]
fn an_http_document_is_not_labelled_with_an_rpc_mode() {
    let dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let file = dir.path().join("probe.httf");
    std::fs::write(
        &file,
        "--- ADDRESS ---\nhttp://127.0.0.1:1\n\n--- ENDPOINT ---\nGET /v1/users\n\n--- ASSERTS ---\n@status() == 200\n",
    )
    .unwrap();

    let mut result = TestResult::pass(file.to_string_lossy().to_string(), 5, Some(3));
    result.family = "httf".to_string();

    let calls = grpctestify::report::kernel::build_kernel_calls(&file.to_string_lossy(), &result)
        .expect("the file has a document");

    assert_eq!(calls[0].rpc_mode, "HTTP", "{:?}", calls[0]);
}
