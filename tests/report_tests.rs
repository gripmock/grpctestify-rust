// Tests for report generators - public API only

use grpctestify::cli::args::ProgressMode;
use grpctestify::report::{Reporter, console::ConsoleReporter, json::JsonReporter};
use grpctestify::state::{TestResult, TestResults, TestStatus};

#[test]
fn test_progress_mode_from_str_dots() {
    // Arrange & Act
    let mode: ProgressMode = "dots".parse().unwrap_or(ProgressMode::Dots);

    // Assert
    assert!(matches!(mode, ProgressMode::Dots));
}

#[test]
fn test_progress_mode_from_str_bar() {
    // Arrange & Act
    let mode: ProgressMode = "bar".parse().unwrap_or(ProgressMode::Dots);

    // Assert
    assert!(matches!(mode, ProgressMode::Bar));
}

#[test]
fn test_progress_mode_from_str_none() {
    // Arrange & Act
    let mode: ProgressMode = "none".parse().unwrap_or(ProgressMode::Dots);

    // Assert
    assert!(matches!(mode, ProgressMode::None));
}

#[test]
fn test_progress_mode_from_str_invalid() {
    // Arrange & Act
    let mode: ProgressMode = "invalid".parse().unwrap_or(ProgressMode::Dots);

    // Assert
    assert!(matches!(mode, ProgressMode::Dots));
}

#[test]
fn test_progress_mode_debug() {
    // Arrange
    let mode = ProgressMode::Dots;

    // Act
    let debug_str = format!("{:?}", mode);

    // Assert
    assert!(debug_str.contains("Dots"));
}

#[test]
fn test_progress_mode_clone() {
    // Arrange
    let mode = ProgressMode::Bar;

    // Act
    let mode_clone = mode;

    // Assert
    assert!(matches!(mode_clone, ProgressMode::Bar));
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
        grpc_duration_ms: None,
        error_message: Some("Skipped due to condition".to_string()),
        execution_time: chrono::Utc::now().timestamp(),
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
        grpc_duration_ms: Some(30),
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
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
        grpc_duration_ms: Some(5),
        error_message: None,
        execution_time: 1700000000,
    });
    results.add(TestResult {
        name: "test_b.gctf".to_string(),
        status: TestStatus::Fail,
        duration_ms: 200,
        grpc_duration_ms: Some(150),
        error_message: Some("Expected 200, got 500".to_string()),
        execution_time: 1700000001,
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
    let reporter = ConsoleReporter::new(ProgressMode::Verbose, 1, env_info);

    // Act & Assert: Should not panic
    reporter.on_test_start("test_verbose.gctf");
    let result = TestResult {
        name: "test_verbose.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        grpc_duration_ms: None,
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
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
    let reporter = ConsoleReporter::new(ProgressMode::Dots, 2, env_info);

    // Act: Emit dots
    let pass1 = TestResult {
        name: "test1.gctf".to_string(),
        status: TestStatus::Pass,
        duration_ms: 10,
        grpc_duration_ms: None,
        error_message: None,
        execution_time: chrono::Utc::now().timestamp(),
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
    let reporter = ConsoleReporter::new(ProgressMode::Verbose, 3, env_info);

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
    let reporter = ConsoleReporter::new(ProgressMode::Verbose, 3, env_info);

    let results = vec![
        TestResult {
            name: "fast.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 5,
            grpc_duration_ms: None,
            error_message: None,
            execution_time: chrono::Utc::now().timestamp(),
        },
        TestResult {
            name: "slow.gctf".to_string(),
            status: TestStatus::Pass,
            duration_ms: 500,
            grpc_duration_ms: None,
            error_message: None,
            execution_time: chrono::Utc::now().timestamp(),
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
