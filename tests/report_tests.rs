// Tests for report generators - public API only

use grpctestify::report::ConsoleMode;
use grpctestify::report::{Reporter, console::ConsoleReporter, json::JsonReporter};
use grpctestify::state::{TestMeta, TestResult, TestResults, TestStatus};

#[test]
fn test_progress_mode_from_str_dots() {
    let mode = ConsoleMode::Dots;
    assert!(matches!(mode, ConsoleMode::Dots));
}

#[test]
fn test_progress_mode_debug() {
    // Arrange
    let mode = ConsoleMode::Dots;

    // Act
    let debug_str = format!("{:?}", mode);

    // Assert
    assert!(debug_str.contains("Dots"));
}

#[test]
fn test_progress_mode_clone() {
    let mode = ConsoleMode::Dots;
    let mode_clone = mode;
    assert!(matches!(mode_clone, ConsoleMode::Dots));
}

#[test]
fn test_junit_reporter_on_suite_end() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("junit.xml");
    let reporter = grpctestify::report::junit::JunitReporter::new(path.clone());
    let results = grpctestify::state::TestResults::new();

    // Act
    let result = reporter.on_suite_end(&results);

    // Assert
    assert!(result.is_ok());
    assert!(path.exists());

    // Verify XML content
    let content = std::fs::read_to_string(&path).expect("Failed to read JUnit file");
    assert!(content.contains("<?xml version=\"1.0\""));
    assert!(content.contains("<testsuites"));
    assert!(content.contains("</testsuites>"));
}

#[test]
fn test_junit_reporter_xml_escaping() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("junit.xml");
    let reporter = grpctestify::report::junit::JunitReporter::new(path.clone());

    // Create results with special characters in error message
    let mut results = grpctestify::state::TestResults::new();
    let fail_result = grpctestify::state::TestResult::fail(
        "test_special.gctf",
        "Error with <special> & \"chars\"".to_string(),
        100,
        None,
    );
    results.add(fail_result);

    // Act
    let result = reporter.on_suite_end(&results);

    // Assert
    assert!(result.is_ok());
    let content = std::fs::read_to_string(&path).expect("Failed to read JUnit file");

    // Verify XML escaping
    assert!(content.contains("&lt;"));
    assert!(content.contains("&gt;"));
    assert!(content.contains("&amp;"));
    assert!(content.contains("&quot;"));
}

#[test]
fn test_junit_reporter_skipped_test() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("junit.xml");
    let reporter = grpctestify::report::junit::JunitReporter::new(path.clone());

    // Create results with skipped test
    let mut results = grpctestify::state::TestResults::new();
    let skip_result = grpctestify::state::TestResult {
        name: "test_skip.gctf".to_string(),
        status: grpctestify::state::TestStatus::Skip,
        duration_ms: 0,
        call_duration_ms: None,
        error_message: Some("Skipped due to condition".to_string()),
        execution_time: chrono::Utc::now().timestamp(),
        meta: grpctestify::state::TestMeta::default(),
    };
    results.add(skip_result);

    // Act
    let result = reporter.on_suite_end(&results);

    // Assert
    assert!(result.is_ok());
    let content = std::fs::read_to_string(&path).expect("Failed to read JUnit file");
    assert!(content.contains("<skipped"));
}

#[test]
fn test_json_reporter_on_suite_end() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("results.json");
    let reporter = JsonReporter::new(path.clone());

    let mut results = TestResults::new();
    let pass_result = TestResult {
        name: "test_pass.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 50,
        call_duration_ms: Some(30),
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
        meta: TestMeta::default(),
    };
    results.add(pass_result);

    // Act
    let result = reporter.on_suite_end(&results);

    // Assert
    assert!(result.is_ok());
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).expect("Failed to read JSON file");
    let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

    // Verify structure
    assert!(json.get("total").is_some());
    assert!(json.get("passed").is_some());
    assert!(json.get("report_context").is_some());
    assert_eq!(json["report_context"]["tool"], "apif");
    assert_eq!(json["total"], 1);
    assert_eq!(json["passed"], 1);
}

#[test]
fn test_json_reporter_with_failure() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("results.json");
    let reporter = JsonReporter::new(path.clone());

    let mut results = TestResults::new();
    let fail_result = TestResult::fail(
        "test_fail.gctf",
        "Assertion mismatch".to_string(),
        100,
        Some(50),
    );
    results.add(fail_result);

    // Act
    let result = reporter.on_suite_end(&results);

    // Assert
    assert!(result.is_ok());
    let content = std::fs::read_to_string(&path).expect("Failed to read JSON file");
    let json: serde_json::Value = serde_json::from_str(&content).expect("Invalid JSON");

    assert_eq!(json["failed"], 1);
}

#[test]
fn test_json_reporter_round_trip() {
    // Arrange
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("roundtrip.json");
    let reporter = JsonReporter::new(path.clone());

    let mut results = TestResults::new();
    results.add(TestResult {
        name: "test_a.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        call_duration_ms: Some(5),
        error_message: None,
        execution_time: 1700000000,
        meta: TestMeta::default(),
    });
    results.add(TestResult {
        name: "test_b.gctf".to_string(),
        status: TestStatus::Fail,
        duration_ms: 200,
        call_duration_ms: Some(150),
        error_message: Some("Expected 200, got 500".to_string()),
        execution_time: 1700000001,
        meta: TestMeta::default(),
    });

    // Act
    let _ = reporter.on_suite_end(&results);

    // Assert: Read back and verify key fields
    let content = std::fs::read_to_string(&path).expect("Failed to read JSON file");
    assert!(content.contains("test_a.gctf"));
    assert!(content.contains("test_b.gctf"));
    assert!(content.contains("Expected 200, got 500"));
}

#[test]
fn test_console_reporter_verbose_mode() {
    // Arrange
    let env_info = grpctestify::report::console::EnvironmentInfo {
        address: "localhost:50051".to_string(),
        parallel_jobs: 1,
        sort_mode: "name".to_string(),
        dry_run: false,
    };
    let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 1, env_info);

    // Act & Assert: Should not panic
    reporter.on_test_start("test_verbose.gctf");
    let result = TestResult {
        name: "test_verbose.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        call_duration_ms: None,
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
        meta: TestMeta::default(),
    };
    reporter.on_test_end("test_verbose.gctf", &result);
}

#[test]
fn test_console_reporter_dots_mode() {
    // Arrange
    let env_info = grpctestify::report::console::EnvironmentInfo {
        address: "localhost:50051".to_string(),
        parallel_jobs: 2,
        sort_mode: "name".to_string(),
        dry_run: false,
    };
    let reporter = ConsoleReporter::new(ConsoleMode::Dots, 2, env_info);

    // Act: Emit dots
    let pass1 = TestResult {
        name: "test1.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        call_duration_ms: None,
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
        meta: TestMeta::default(),
    };
    let fail1 = TestResult::fail("test2.gctf", "error".to_string(), 20, None);
    reporter.on_test_end("test1.gctf", &pass1);
    reporter.on_test_end("test2.gctf", &fail1);
}

#[test]
fn test_console_reporter_print_summary() {
    // Arrange
    let env_info = grpctestify::report::console::EnvironmentInfo {
        address: "localhost:50051".to_string(),
        parallel_jobs: 1,
        sort_mode: "name".to_string(),
        dry_run: true,
    };
    let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 3, env_info);

    // Act: Should not panic
    reporter.print_summary(
        3,
        2,
        1,
        0,
        150,
        &["test_fail.gctf (20ms)\n      Error: assertion failed".to_string()],
        &grpctestify::state::ExecutionMetrics::default(),
    );
}

#[test]
fn test_console_reporter_print_slowest_tests() {
    // Arrange
    let env_info = grpctestify::report::console::EnvironmentInfo {
        address: "localhost:50051".to_string(),
        parallel_jobs: 1,
        sort_mode: "name".to_string(),
        dry_run: false,
    };
    let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 3, env_info);

    let results = vec![
        TestResult {
            name: "fast.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 5,
            call_duration_ms: None,
            error_message: None,
            execution_time: chrono::Utc::now().timestamp(),
            meta: TestMeta::default(),
        },
        TestResult {
            name: "slow.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 500,
            call_duration_ms: None,
            error_message: None,
            execution_time: chrono::Utc::now().timestamp(),
            meta: TestMeta::default(),
        },
    ];

    // Act: Should not panic
    reporter.print_slowest_tests(&results, 2);
}

#[test]
fn test_coverage_collector_new() {
    let collector = grpctestify::report::coverage::CoverageCollector::new();
    let report = collector.generate_json_report();
    assert_eq!(report.files.len(), 0);
    assert_eq!(report.messages.len(), 0);
    assert_eq!(report.summary.covered, 0);
    assert_eq!(report.summary.total, 0);
}

#[test]
fn test_coverage_collector_record_call() {
    let collector = grpctestify::report::coverage::CoverageCollector::new();
    collector.record_call("TestService", "TestMethod");
    collector.record_call("TestService", "TestMethod");
    collector.record_call("TestService", "OtherMethod");

    let report = collector.generate_json_report();
    assert_eq!(report.summary.covered, 0);
    assert_eq!(report.summary.total, 0);
}

#[test]
fn test_coverage_collector_extract_fields() {
    let collector = grpctestify::report::coverage::CoverageCollector::new();
    let json = serde_json::json!({
        "name": "test",
        "age": 30,
        "nested": {
            "key": "value"
        }
    });
    collector.record_fields_from_json("TestMessage", &json);

    let report = collector.generate_json_report();
    assert_eq!(report.messages.len(), 0);
}

#[test]
fn test_coverage_stats() {
    let stats = grpctestify::report::coverage::CoverageStats {
        covered: 5,
        total: 10,
    };
    assert_eq!(stats.covered, 5);
    assert_eq!(stats.total, 10);
}

#[test]
fn test_coverage_file() {
    let file = grpctestify::report::coverage::CoverageFile {
        uri: "grpc://test.Service".to_string(),
        statements: grpctestify::report::coverage::CoverageStats {
            covered: 2,
            total: 5,
        },
        branches: None,
        functions: Some(grpctestify::report::coverage::CoverageStats {
            covered: 2,
            total: 5,
        }),
        fields: None,
    };
    assert_eq!(file.uri, "grpc://test.Service");
    assert_eq!(file.functions.as_ref().unwrap().covered, 2);
}

#[test]
fn test_message_field_coverage() {
    let msg = grpctestify::report::coverage::MessageFieldCoverage {
        message_type: "User".to_string(),
        covered_fields: vec!["name".to_string(), "email".to_string()],
        total_fields: 3,
    };
    assert_eq!(msg.message_type, "User");
    assert_eq!(msg.covered_fields.len(), 2);
}

#[test]
fn test_coverage_text_report_empty() {
    let collector = grpctestify::report::coverage::CoverageCollector::new();
    let text = collector.generate_text_report();
    assert!(text.contains("No services found"));
}

#[test]
fn test_junit_reporter_tags_in_properties() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("junit.xml");
    let reporter = grpctestify::report::junit::JunitReporter::new(path.clone());

    let mut results = grpctestify::state::TestResults::new();
    let meta = TestMeta {
        tags: vec!["smoke".to_string(), "integration".to_string()],
        ..TestMeta::default()
    };
    results.add(TestResult {
        name: "test_tagged.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        call_duration_ms: Some(5),
        error_message: None,
        execution_time: 1700000000,
        meta,
    });

    let result = reporter.on_suite_end(&results);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).expect("Failed to read JUnit file");
    assert!(content.contains(r#"property name="tag" value="smoke""#));
    assert!(content.contains(r#"property name="tag" value="integration""#));
    assert!(content.contains("<properties>"));
}

#[test]
fn test_json_reporter_includes_meta() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let path = temp_dir.path().join("results.json");
    let reporter = JsonReporter::new(path.clone());

    let mut results = TestResults::new();
    let meta = TestMeta {
        name: Some("my test suite".to_string()),
        tags: vec!["smoke".to_string()],
        owner: Some("backend-qa".to_string()),
        ..TestMeta::default()
    };
    results.add(TestResult {
        name: "test.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        call_duration_ms: Some(5),
        error_message: None,
        execution_time: 1700000000,
        meta,
    });

    let result = reporter.on_suite_end(&results);
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).expect("Failed to read JSON file");
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let result_meta = &json["results"][0]["meta"];
    assert_eq!(result_meta["name"], "my test suite");
    assert_eq!(result_meta["owner"], "backend-qa");
    assert!(
        result_meta["tags"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("smoke"))
    );
}
