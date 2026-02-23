// Test result structures

use crate::state::TestStatus;
use serde::Serialize;

/// Test result
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestResult {
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub grpc_duration_ms: Option<u64>,
    pub error_message: Option<String>,
    pub execution_time: i64,
}

impl TestResult {
    /// Create a pass result
    pub fn pass(name: impl Into<String>, duration_ms: u64, grpc_duration_ms: Option<u64>) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Pass,
            duration_ms,
            grpc_duration_ms,
            error_message: None,
            execution_time: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a fail result
    pub fn fail(
        name: impl Into<String>,
        error_message: String,
        duration_ms: u64,
        grpc_duration_ms: Option<u64>,
    ) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Fail,
            duration_ms,
            grpc_duration_ms,
            error_message: Some(error_message),
            execution_time: chrono::Utc::now().timestamp(),
        }
    }

    /// Create a skip result
    #[allow(dead_code)]
    pub fn skip(name: impl Into<String>, reason: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Skip,
            duration_ms,
            grpc_duration_ms: None,
            error_message: Some(reason.into()),
            execution_time: chrono::Utc::now().timestamp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_result_pass() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.grpc_duration_ms, Some(50));
        assert!(result.error_message.is_none());
    }

    #[test]
    fn test_test_result_pass_no_grpc() {
        let result = TestResult::pass("test.gctf", 100, None);
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Pass);
        assert!(result.grpc_duration_ms.is_none());
    }

    #[test]
    fn test_test_result_fail() {
        let result = TestResult::fail("test.gctf", "error message".to_string(), 100, Some(50));
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.grpc_duration_ms, Some(50));
        assert_eq!(result.error_message, Some("error message".to_string()));
    }

    #[test]
    fn test_test_result_skip() {
        let result = TestResult::skip("test.gctf", "skipped reason".to_string(), 100);
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Skip);
        assert_eq!(result.duration_ms, 100);
        assert!(result.grpc_duration_ms.is_none());
        assert_eq!(result.error_message, Some("skipped reason".to_string()));
    }

    #[test]
    fn test_test_result_clone() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let cloned = result.clone();
        assert_eq!(result.name, cloned.name);
        assert_eq!(result.status, cloned.status);
    }

    #[test]
    fn test_test_result_debug() {
        let result = TestResult::pass("test.gctf", 100, Some(50));
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("test.gctf"));
        assert!(debug_str.contains("Pass"));
    }
}
