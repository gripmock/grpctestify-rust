use anyhow::Result;
use apif_state::{TestResult, TestResults};
use serde_json::json;
use std::io::{self, Write};
use std::sync::Once;

use super::Reporter;

pub struct StreamingJsonReporter {
    /// Guarantees `suite_start` is emitted exactly once, and that the emit
    /// completes before any concurrent caller proceeds — so no `test_start`
    /// can ever race ahead of `suite_start` under parallel execution.
    suite_started: Once,
    test_count: usize,
    #[cfg(test)]
    captured: std::sync::Mutex<Vec<String>>,
}

impl StreamingJsonReporter {
    pub fn new(test_count: usize) -> Self {
        Self {
            suite_started: Once::new(),
            test_count,
            #[cfg(test)]
            captured: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Emit `suite_start` exactly once, blocking concurrent callers until it has
    /// been written. `Once::call_once` provides the ordering guarantee.
    fn ensure_suite_started(&self) {
        self.suite_started.call_once(|| {
            self.emit(&json!({
                "event": "suite_start",
                "testCount": self.test_count,
                "timestamp": apif_cfg_runtime::now_rfc3339(),
            }));
        });
    }

    fn emit(&self, event: &serde_json::Value) {
        match serde_json::to_string(event) {
            Ok(s) => self.write_line(&s),
            Err(e) => {
                tracing::warn!("Failed to serialize streaming event: {e}");
            }
        }
    }

    fn write_line(&self, s: &str) {
        #[cfg(test)]
        if let Ok(mut cap) = self.captured.lock() {
            cap.push(s.to_string());
        }
        let mut stdout = io::stdout().lock();
        if let Err(e) = writeln!(stdout, "{}", s) {
            tracing::warn!("Failed to write streaming JSON to stdout: {e}");
        }
        if let Err(e) = stdout.flush() {
            tracing::warn!("Failed to flush stdout: {e}");
        }
    }
}

impl Reporter for StreamingJsonReporter {
    fn on_test_start(&self, test_name: &str) {
        self.ensure_suite_started();

        self.emit(&json!({
            "event": "test_start",
            "testId": test_name,
            "timestamp": apif_cfg_runtime::now_rfc3339()
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
            "timestamp": apif_cfg_runtime::now_rfc3339()
        });

        if let Some(msg) = &result.error_message {
            event["message"] = json!(msg);
        }

        if let Some(grpc_ms) = result.call_duration_ms {
            event["grpcDuration"] = json!(grpc_ms);
        }

        // Per-assertion outcomes (line/expression/passed, expected/actual on
        // failure) so an IDE consuming this stream can render the same diff
        // the verbose console and other reporters show, without a second pass.
        if !result.assertions.is_empty() {
            event["assertions"] = json!(
                result
                    .assertions
                    .iter()
                    .map(|a| {
                        let mut j = json!({
                            "line": a.line,
                            "expression": a.expression,
                            "passed": a.passed,
                        });
                        if let Some(expected) = &a.expected {
                            j["expected"] = json!(expected);
                        }
                        if let Some(actual) = &a.actual {
                            j["actual"] = json!(actual);
                        }
                        if let Some(message) = &a.message {
                            j["message"] = json!(message);
                        }
                        j
                    })
                    .collect::<Vec<_>>()
            );
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
            "timestamp": apif_cfg_runtime::now_rfc3339()
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
    fn streaming_reporter_new() {
        let reporter = StreamingJsonReporter::new(5);
        assert_eq!(reporter.test_count, 5);
        assert!(!reporter.suite_started.is_completed());
    }

    #[test]
    fn streaming_reporter_lifecycle() {
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
        r.expect("on_suite_end must succeed");
    }

    #[test]
    fn streaming_reporter_on_test_end_with_error_message() {
        let reporter = StreamingJsonReporter::new(1);
        reporter.on_test_start("test");

        let result = TestResult::fail("test.gctf", "something broke".into(), 150, None);
        reporter.on_test_end("test", &result);

        let results = TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }

    #[test]
    fn streaming_test_end_includes_assertion_outcomes() {
        use apif_state::AssertionRecord;
        let reporter = StreamingJsonReporter::new(1);
        let result = TestResult::fail("t.gctf", "1 assertion failed".into(), 5, None)
            .with_assertions(vec![AssertionRecord {
                line: 4,
                expression: ".ok == true".into(),
                passed: false,
                elapsed_ms: 1,
                message: Some("mismatch".into()),
                endpoint: None,
                expected: Some("true".into()),
                actual: Some("false".into()),
            }]);
        reporter.on_test_end("t", &result);

        let cap = reporter.captured.lock().unwrap();
        let line = cap
            .iter()
            .find(|l| l.contains("test_fail"))
            .expect("test_fail event");
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        let assertions = event["assertions"].as_array().expect("assertions array");
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0]["line"], 4);
        assert_eq!(assertions[0]["passed"], false);
        assert_eq!(assertions[0]["expected"], "true");
        assert_eq!(assertions[0]["actual"], "false");
    }

    #[test]
    fn streaming_test_end_omits_assertions_when_none_recorded() {
        let reporter = StreamingJsonReporter::new(1);
        let result = TestResult::pass("t.gctf", 5, None);
        reporter.on_test_end("t", &result);

        let cap = reporter.captured.lock().unwrap();
        let line = cap
            .iter()
            .find(|l| l.contains("test_pass"))
            .expect("test_pass event");
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(event.get("assertions").is_none());
    }

    #[test]
    fn streaming_reporter_suite_start_once() {
        let reporter = StreamingJsonReporter::new(1);
        reporter.on_test_start("t1");
        assert!(reporter.suite_started.is_completed());
        // Subsequent calls should not re-emit suite_start
        reporter.on_test_start("t2");
        let cap = reporter.captured.lock().unwrap();
        assert_eq!(
            cap.iter().filter(|l| l.contains("suite_start")).count(),
            1,
            "suite_start must be emitted exactly once: {cap:?}"
        );
    }

    #[test]
    fn streaming_suite_start_precedes_test_start_under_parallelism() {
        use std::sync::Arc;
        let reporter = Arc::new(StreamingJsonReporter::new(16));
        let mut handles = Vec::new();
        for i in 0..16 {
            let r = Arc::clone(&reporter);
            handles.push(std::thread::spawn(move || {
                r.on_test_start(&format!("test{i}"));
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let cap = reporter.captured.lock().unwrap();
        // suite_start emitted exactly once...
        assert_eq!(
            cap.iter().filter(|l| l.contains("suite_start")).count(),
            1,
            "suite_start must be emitted exactly once: {cap:?}"
        );
        // ...and strictly before the first test_start.
        let suite_pos = cap.iter().position(|l| l.contains("suite_start")).unwrap();
        let first_test_pos = cap.iter().position(|l| l.contains("test_start")).unwrap();
        assert!(
            suite_pos < first_test_pos,
            "suite_start must precede every test_start: {cap:?}"
        );
    }
}
