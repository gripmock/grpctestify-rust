// State module - Test state management
// Centralized management of test results and metrics

pub mod metrics;
pub mod result;

pub use metrics::ExecutionMetrics;
pub use result::TestResult;

use serde::Serialize;

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
    /// Create new test results
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

    /// Add a test result
    pub fn add(&mut self, result: TestResult) {
        if let Some(duration) = result.grpc_duration_ms {
            self.metrics.grpc_total_duration_ms += duration;
            self.metrics.grpc_calls += 1;
        }

        self.results.push(result.clone());
        self.total += 1;

        match result.status {
            TestStatus::Pass => self.passed += 1,
            TestStatus::Fail => self.failed += 1,
            TestStatus::Skip => self.skipped += 1,
        }
    }

    /// Get total tests
    pub fn total(&self) -> usize {
        self.total
    }

    /// Get passed tests
    pub fn passed(&self) -> usize {
        self.passed
    }

    /// Get failed tests
    pub fn failed(&self) -> usize {
        self.failed
    }

    /// Get skipped tests
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Get test result by index
    pub fn get(&self, index: usize) -> Option<&TestResult> {
        self.results.get(index)
    }

    /// Get all results
    pub fn all(&self) -> &[TestResult] {
        &self.results
    }

    /// Check if all tests passed
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Get pass rate
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    /// Get execution metrics
    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    /// Reset results
    pub fn reset(&mut self) {
        self.total = 0;
        self.passed = 0;
        self.failed = 0;
        self.skipped = 0;
        self.results.clear();
        self.metrics = ExecutionMetrics::default();
    }
}

impl ExecutionMetrics {
    /// Update execution time
    #[allow(dead_code)]
    pub fn update_time(&mut self) {
        self.end_time = chrono::Utc::now().timestamp();
        self.total_duration_ms = (self.end_time - self.start_time) as u64;
    }
}

/// Test status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TestStatus {
    Pass,
    Fail,
    #[allow(dead_code)]
    Skip,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_results_new() {
        let results = TestResults::new();
        assert_eq!(results.total(), 0);
        assert_eq!(results.passed(), 0);
        assert_eq!(results.failed(), 0);
        assert_eq!(results.skipped(), 0);
        assert!(results.all_passed());
        assert_eq!(results.pass_rate(), 0.0);
    }

    #[test]
    fn test_test_results_default() {
        let results = TestResults::default();
        assert_eq!(results.total(), 0);
    }

    #[test]
    fn test_test_results_add_pass() {
        let mut results = TestResults::new();
        let result = TestResult::pass("test1.gctf", 100, Some(50));
        results.add(result);
        assert_eq!(results.total(), 1);
        assert_eq!(results.passed(), 1);
        assert_eq!(results.failed(), 0);
        assert!(results.all_passed());
        assert_eq!(results.pass_rate(), 100.0);
    }

    #[test]
    fn test_test_results_add_fail() {
        let mut results = TestResults::new();
        let result = TestResult::fail("test1.gctf", "error".to_string(), 100, Some(50));
        results.add(result);
        assert_eq!(results.total(), 1);
        assert_eq!(results.passed(), 0);
        assert_eq!(results.failed(), 1);
        assert!(!results.all_passed());
        assert_eq!(results.pass_rate(), 0.0);
    }

    #[test]
    fn test_test_results_mixed() {
        let mut results = TestResults::new();
        results.add(TestResult::pass("test1.gctf", 100, Some(50)));
        results.add(TestResult::pass("test2.gctf", 100, Some(50)));
        results.add(TestResult::fail(
            "test3.gctf",
            "error".to_string(),
            100,
            Some(50),
        ));
        assert_eq!(results.total(), 3);
        assert_eq!(results.passed(), 2);
        assert_eq!(results.failed(), 1);
        assert!(!results.all_passed());
        assert!((results.pass_rate() - 66.67).abs() < 0.01);
    }

    #[test]
    fn test_test_results_get() {
        let mut results = TestResults::new();
        results.add(TestResult::pass("test1.gctf", 100, Some(50)));
        assert!(results.get(0).is_some());
        assert!(results.get(1).is_none());
    }

    #[test]
    fn test_test_results_all() {
        let mut results = TestResults::new();
        results.add(TestResult::pass("test1.gctf", 100, Some(50)));
        results.add(TestResult::pass("test2.gctf", 100, Some(50)));
        assert_eq!(results.all().len(), 2);
    }

    #[test]
    fn test_test_results_reset() {
        let mut results = TestResults::new();
        results.add(TestResult::pass("test1.gctf", 100, Some(50)));
        results.add(TestResult::fail(
            "test2.gctf",
            "error".to_string(),
            100,
            Some(50),
        ));
        results.reset();
        assert_eq!(results.total(), 0);
        assert_eq!(results.passed(), 0);
        assert_eq!(results.failed(), 0);
        assert!(results.all_passed());
    }

    #[test]
    fn test_test_results_metrics() {
        let mut results = TestResults::new();
        results.add(TestResult::pass("test1.gctf", 100, Some(50)));
        results.add(TestResult::pass("test2.gctf", 100, Some(30)));
        let metrics = results.metrics();
        assert_eq!(metrics.grpc_calls, 2);
        assert_eq!(metrics.grpc_total_duration_ms, 80);
    }

    #[test]
    fn test_execution_metrics_update_time() {
        let mut metrics = ExecutionMetrics::default();
        let start = metrics.start_time;
        std::thread::sleep(std::time::Duration::from_millis(150));
        metrics.update_time();
        assert!(metrics.end_time >= start);
    }
}
