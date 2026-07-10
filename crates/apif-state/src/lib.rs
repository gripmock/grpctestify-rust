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

    pub fn get(&self, index: usize) -> Option<&TestResult> {
        self.results.get(index)
    }

    pub fn all(&self) -> &[TestResult] {
        &self.results
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    pub fn reset(&mut self) {
        self.total = 0;
        self.passed = 0;
        self.failed = 0;
        self.skipped = 0;
        self.results.clear();
        self.metrics = ExecutionMetrics::default();
    }

    pub fn merge(&mut self, other: TestResults) {
        self.total += other.total;
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.results.extend(other.results);
        self.metrics.merge(other.metrics);
    }
}
