// JSON reporter - outputs test results to a JSON file

use super::Reporter;
use crate::state::{TestResult, TestResults};
use anyhow::{Context, Result};
use std::fs::File;
use std::path::PathBuf;

/// JSON reporter
pub struct JsonReporter {
    output_path: PathBuf,
    // We don't need to accumulate results here because on_suite_end receives the full TestResults
    // However, if we wanted to stream results (e.g. ndjson), we would write in on_test_end
    // For now, let's just write the full report at the end.
}

impl JsonReporter {
    /// Create new JSON reporter
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

impl Reporter for JsonReporter {
    fn on_test_start(&self, _test_name: &str) {
        // No-op for JSON file reporter
    }

    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {
        // No-op for standard JSON report (unless streaming)
    }

    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let file = File::create(&self.output_path).with_context(|| {
            format!(
                "Failed to create JSON report file: {}",
                self.output_path.display()
            )
        })?;

        serde_json::to_writer_pretty(file, results)
            .context("Failed to serialize test results to JSON")?;

        Ok(())
    }
}
