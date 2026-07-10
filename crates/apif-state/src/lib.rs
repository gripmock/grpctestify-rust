pub mod metrics;
pub mod result;

pub use metrics::ExecutionMetrics;
pub use result::{TestMeta, TestResult};

use serde::Serialize;

/// Test status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

/// Test results storage
#[derive(Debug, Clone, Serialize)]
pub struct TestResults {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    results: Vec<TestResult>,
    pub metrics: ExecutionMetrics,
}

impl Default for TestResults {
    fn default() -> Self {
        Self::new()
    }
}

impl TestResults {
    pub fn new() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            results: Vec::new(),
            metrics: ExecutionMetrics::default(),
        }
    }

    pub fn add(&mut self, result: TestResult) {
        if let Some(duration) = result.call_duration_ms {
            self.metrics.total_rpc_ms += duration;
            self.metrics.rpc_calls += 1;
        }
        self.results.push(result.clone());
        self.total += 1;
        match result.status {
            TestStatus::Pass => self.passed += 1,
            TestStatus::Fail => self.failed += 1,
            TestStatus::Skip => self.skipped += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.total
    }
    pub fn passed(&self) -> usize {
        self.passed
    }
    pub fn failed(&self) -> usize {
        self.failed
    }
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    pub fn all(&self) -> &[TestResult] {
        &self.results
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_results_new() {
        let r = TestResults::new();
        assert_eq!(r.total(), 0);
        assert_eq!(r.passed(), 0);
        assert_eq!(r.failed(), 0);
        assert_eq!(r.skipped(), 0);
        assert!(r.all().is_empty());
        assert!(r.all_passed());
    }

    #[test]
    fn test_test_results_add_pass() {
        let mut r = TestResults::new();
        r.add(TestResult::pass("t.gctf", 100, Some(50)));
        assert_eq!(r.total(), 1);
        assert_eq!(r.passed(), 1);
        assert_eq!(r.failed(), 0);
        assert_eq!(r.skipped(), 0);
        assert!(r.all_passed());
        assert_eq!(r.all().len(), 1);
        assert_eq!(r.metrics().rpc_calls, 1);
    }

    #[test]
    fn test_test_results_add_fail() {
        let mut r = TestResults::new();
        r.add(TestResult::fail("t.gctf", "err".into(), 100, None));
        assert_eq!(r.total(), 1);
        assert_eq!(r.passed(), 0);
        assert_eq!(r.failed(), 1);
        assert!(!r.all_passed());
    }

    #[test]
    fn test_test_results_add_skip() {
        let mut r = TestResults::new();
        r.add(TestResult {
            name: "t.gctf".into(),
            status: TestStatus::Skip,
            duration_ms: 0,
            call_duration_ms: None,
            error_message: None,
            execution_time: 0,
            meta: TestMeta::default(),
        });
        assert_eq!(r.total(), 1);
        assert_eq!(r.skipped(), 1);
    }

    #[test]
    fn test_test_results_mixed() {
        let mut r = TestResults::new();
        r.add(TestResult::pass("a.gctf", 10, None));
        r.add(TestResult::pass("b.gctf", 20, None));
        r.add(TestResult::fail("c.gctf", "err".into(), 30, None));
        assert_eq!(r.total(), 3);
        assert_eq!(r.passed(), 2);
        assert_eq!(r.failed(), 1);
        assert!(!r.all_passed());
        assert_eq!(r.all().len(), 3);
    }

    #[test]
    fn test_execution_metrics_default() {
        let m = ExecutionMetrics::default();
        assert_eq!(m.total_duration_ms, 0);
        assert_eq!(m.rpc_calls, 0);
        assert_eq!(m.parallel_jobs, 1);
    }
}
