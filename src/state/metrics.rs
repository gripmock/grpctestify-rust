// Execution metrics

use chrono::Utc;
use serde::Serialize;

/// Execution metrics
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionMetrics {
    pub total_duration_ms: u64,
    pub start_time: i64,
    pub end_time: i64,
    pub grpc_calls: u64,
    pub grpc_total_duration_ms: u64,
    pub parallel_jobs: usize,
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self {
            total_duration_ms: 0,
            start_time: Utc::now().timestamp(),
            end_time: 0,
            grpc_calls: 0,
            grpc_total_duration_ms: 0,
            parallel_jobs: 1,
        }
    }
}
