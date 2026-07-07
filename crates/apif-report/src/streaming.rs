use apif_state::{TestResult, TestResults};
use anyhow::Result;
use serde_json::json;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use super::Reporter;

pub struct StreamingJsonReporter {
    suite_started: AtomicBool,
    test_count: usize,
}

impl StreamingJsonReporter {
    pub fn new(test_count: usize) -> Self {
        Self {
            suite_started: AtomicBool::new(false),
            test_count,
        }
    }

    fn emit(&self, event: &serde_json::Value) {
        let mut stdout = io::stdout().lock();
        match serde_json::to_string(event) {
            Ok(s) => {
                if let Err(e) = writeln!(stdout, "{}", s) {
                    tracing::warn!("Failed to write streaming JSON to stdout: {e}");
                }
                if let Err(e) = stdout.flush() {
                    tracing::warn!("Failed to flush stdout: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize streaming event: {e}");
            }
        }
    }
}

impl Reporter for StreamingJsonReporter {
    fn on_test_start(&self, test_name: &str) {
        if !self.suite_started.swap(true, Ordering::SeqCst) {
            self.emit(&json!({
                "event": "suite_start",
                "testCount": self.test_count,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }));
        }

        self.emit(&json!({
            "event": "test_start",
            "testId": test_name,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));
    }

    fn on_test_end(&self, test_name: &str, result: &TestResult) {
        let event_type = match result.status {
            apif_state::TestStatus::Pass => "test_pass",
            apif_state::TestStatus::Fail => "test_fail",
            apif_state::TestStatus::Skip => "test_skip",
        };

        let mut event = json!({
            "event": event_type,
            "testId": test_name,
            "duration": result.duration_ms,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        if let Some(msg) = &result.error_message {
            event["message"] = json!(msg);
        }

        if let Some(grpc_ms) = result.call_duration_ms {
            event["grpcDuration"] = json!(grpc_ms);
        }

        self.emit(&event);
    }

    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        self.emit(&json!({
            "event": "suite_end",
            "summary": {
                "total": results.total(),
                "passed": results.passed(),
                "failed": results.failed(),
                "skipped": results.skipped(),
                "duration": results.metrics.total_duration_ms
            },
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));

        Ok(())
    }
}
