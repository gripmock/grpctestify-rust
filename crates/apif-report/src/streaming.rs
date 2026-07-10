use anyhow::Result;
use apif_state::{TestResult, TestResults};
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
                "timestamp": cfg_runtime::now_rfc3339(),
            }));
        }

        self.emit(&json!({
            "event": "test_start",
            "testId": test_name,
            "timestamp": cfg_runtime::now_rfc3339()
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
            "timestamp": cfg_runtime::now_rfc3339()
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
            "timestamp": cfg_runtime::now_rfc3339()
        }));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reporter;
    use apif_state::TestResult;

    #[test]
    fn test_streaming_reporter_new() {
        let reporter = StreamingJsonReporter::new(5);
        assert_eq!(reporter.test_count, 5);
        assert!(!reporter.suite_started.load(Ordering::SeqCst));
    }

    #[test]
    fn test_streaming_reporter_lifecycle() {
        let reporter = StreamingJsonReporter::new(2);
        // These should not panic — emit writes to stdout but swallows errors
        reporter.on_test_start("test1");
        reporter.on_test_start("test2");

        let result = TestResult::pass("test1.gctf", 100, Some(50));
        reporter.on_test_end("test1", &result);

        let result = TestResult::fail("test2.gctf", "error".to_string(), 200, Some(100));
        reporter.on_test_end("test2", &result);

        let results = TestResults::new();
        let r = reporter.on_suite_end(&results);
        assert!(r.is_ok());
    }

    #[test]
    fn test_streaming_reporter_on_test_end_with_error_message() {
        let reporter = StreamingJsonReporter::new(1);
        reporter.on_test_start("test");

        let result = TestResult::fail("test.gctf", "something broke".into(), 150, None);
        reporter.on_test_end("test", &result);

        let results = TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }

    #[test]
    fn test_streaming_reporter_suite_start_once() {
        let reporter = StreamingJsonReporter::new(1);
        // First call sets suite_started
        reporter.on_test_start("t1");
        assert!(reporter.suite_started.load(Ordering::SeqCst));
        // Subsequent calls should not re-emit suite_start
        reporter.on_test_start("t2");
    }
}
