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
    #[allow(dead_code)]
    pub fn get(&self, index: usize) -> Option<&TestResult> {
        self.results.get(index)
    }

    /// Get all results
    #[allow(dead_code)]
    pub fn all(&self) -> &[TestResult] {
        &self.results
    }

    /// Check if all tests passed
    #[allow(dead_code)]
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Get pass rate
    #[allow(dead_code)]
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    /// Get execution metrics
    #[allow(dead_code)]
    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    /// Reset results
    #[allow(dead_code)]
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
