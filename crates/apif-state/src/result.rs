// Test result structures

use crate::TestStatus;
use serde::Serialize;

/// Metadata extracted from META section for test reports
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(default)]
pub struct TestMeta {
    /// Display name (from META.name, falls back to filename)
    pub name: Option<String>,
    /// Test summary
    pub summary: Option<String>,
    /// Test tags
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Test owner
    pub owner: Option<String>,
    /// Related links
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl TestMeta {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.summary.is_none() && self.tags.is_empty()
    }

    pub fn from_file_meta(file_meta: &apif_ast::FileMeta) -> Self {
        Self {
            name: file_meta.name.clone(),
            summary: file_meta.summary.clone(),
            tags: file_meta.tags.clone(),
            owner: file_meta.owner.clone(),
            links: file_meta.links.clone(),
        }
    }
}

/// Test result
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestResult {
    /// File path (used as fallback name)
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub call_duration_ms: Option<u64>,
    pub error_message: Option<String>,
    pub execution_time: i64,
    /// Test metadata from META section
    #[serde(default, skip_serializing_if = "TestMeta::is_empty")]
    pub meta: TestMeta,
}

impl TestResult {
    /// Create a pass result
    pub fn pass(name: impl Into<String>, duration_ms: u64, call_duration_ms: Option<u64>) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Pass,
            duration_ms,
            call_duration_ms,
            error_message: None,
            execution_time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            meta: TestMeta::default(),
        }
    }

    /// Create a pass result with metadata
    pub fn pass_with_meta(
        name: impl Into<String>,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
        meta: TestMeta,
    ) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Pass,
            duration_ms,
            call_duration_ms,
            error_message: None,
            execution_time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            meta,
        }
    }

    /// Create a fail result
    pub fn fail(
        name: impl Into<String>,
        error_message: String,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
    ) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Fail,
            duration_ms,
            call_duration_ms,
            error_message: Some(error_message),
            execution_time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            meta: TestMeta::default(),
        }
    }

    /// Create a fail result with metadata
    pub fn fail_with_meta(
        name: impl Into<String>,
        error_message: String,
        duration_ms: u64,
        call_duration_ms: Option<u64>,
        meta: TestMeta,
    ) -> Self {
        Self {
            name: name.into(),
            status: TestStatus::Fail,
            duration_ms,
            call_duration_ms,
            error_message: Some(error_message),
            execution_time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            meta,
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
        assert_eq!(result.call_duration_ms, Some(50));
        assert!(result.error_message.is_none());
    }

    #[test]
    fn test_test_result_pass_no_grpc() {
        let result = TestResult::pass("test.gctf", 100, None);
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Pass);
        assert!(result.call_duration_ms.is_none());
    }

    #[test]
    fn test_test_result_fail() {
        let result = TestResult::fail("test.gctf", "error message".to_string(), 100, Some(50));
        assert_eq!(result.name, "test.gctf");
        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.duration_ms, 100);
        assert_eq!(result.call_duration_ms, Some(50));
        assert_eq!(result.error_message, Some("error message".to_string()));
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
