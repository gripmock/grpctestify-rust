// Execution metrics

use serde::Serialize;

/// Execution metrics
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionMetrics {
    pub total_duration_ms: u64,
    pub start_time: i64,
    pub end_time: i64,
    pub rpc_calls: u64,
    pub total_rpc_ms: u64,
    pub parallel_jobs: usize,
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self {
            total_duration_ms: 0,
            start_time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            end_time: 0,
            rpc_calls: 0,
            total_rpc_ms: 0,
            parallel_jobs: 1,
        }
    }
}

impl ExecutionMetrics {
    pub fn merge(&mut self, other: ExecutionMetrics) {
        self.total_duration_ms = self.total_duration_ms.max(other.total_duration_ms);
        self.start_time = self.start_time.min(other.start_time);
        self.end_time = self.end_time.max(other.end_time);
        self.rpc_calls += other.rpc_calls;
        self.total_rpc_ms += other.total_rpc_ms;
        self.parallel_jobs = self.parallel_jobs.max(other.parallel_jobs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_metrics_default() {
        let metrics = ExecutionMetrics::default();
        assert_eq!(metrics.total_duration_ms, 0);
        assert_eq!(metrics.rpc_calls, 0);
        assert_eq!(metrics.total_rpc_ms, 0);
        assert_eq!(metrics.parallel_jobs, 1);
        assert_eq!(metrics.end_time, 0);
    }

    #[test]
    fn test_execution_metrics_clone() {
        let metrics = ExecutionMetrics::default();
        let cloned = metrics.clone();
        assert_eq!(metrics.total_duration_ms, cloned.total_duration_ms);
        assert_eq!(metrics.rpc_calls, cloned.rpc_calls);
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
        assert!(json.contains("rpc_calls"));
    }
}
