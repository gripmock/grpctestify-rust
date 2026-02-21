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
