// Tests for report generators - public API only

use grpctestify::cli::args::ProgressMode;
use grpctestify::report::Reporter;

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
    let mode_clone = mode.clone();

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
