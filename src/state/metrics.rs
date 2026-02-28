// Execution metrics

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
            start_time: crate::time::now_timestamp(),
            end_time: 0,
            grpc_calls: 0,
            grpc_total_duration_ms: 0,
            parallel_jobs: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_metrics_default() {
        let metrics = ExecutionMetrics::default();
        assert_eq!(metrics.total_duration_ms, 0);
        assert_eq!(metrics.grpc_calls, 0);
        assert_eq!(metrics.grpc_total_duration_ms, 0);
        assert_eq!(metrics.parallel_jobs, 1);
        assert_eq!(metrics.end_time, 0);
    }

    #[test]
    fn test_execution_metrics_clone() {
        let metrics = ExecutionMetrics::default();
        let cloned = metrics.clone();
        assert_eq!(metrics.total_duration_ms, cloned.total_duration_ms);
        assert_eq!(metrics.grpc_calls, cloned.grpc_calls);
    }

    #[test]
    fn test_execution_metrics_debug() {
        let metrics = ExecutionMetrics::default();
        let debug_str = format!("{:?}", metrics);
        assert!(debug_str.contains("ExecutionMetrics"));
    }

    #[test]
    fn test_execution_metrics_serialize() {
        let metrics = ExecutionMetrics::default();
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("total_duration_ms"));
        assert!(json.contains("grpc_calls"));
    }
}
