use std::cmp::Reverse;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    Dots,
    Verbose,
    Silent,
}

use apif_state::{TestResult, TestStatus};

const SLOW_TEST_THRESHOLD_MS: u64 = 500;

#[derive(Debug, Clone)]
pub struct EnvironmentInfo {
    pub address: String,
    pub parallel_jobs: usize,
    pub sort_mode: String,
    pub dry_run: bool,
    pub warnings: Vec<String>,
}

pub struct ConsoleReporter {
    mode: ConsoleMode,
    env_info: EnvironmentInfo,
    dots_lock: Mutex<()>,
    dots_count: AtomicUsize,
    results: Mutex<Vec<TestResult>>,
}

impl ConsoleReporter {
    pub fn new(mode: ConsoleMode, _total_tests: u64, env_info: EnvironmentInfo) -> Self {
        Self {
            mode,
            env_info,
            dots_lock: Mutex::new(()),
            dots_count: AtomicUsize::new(0),
            results: Mutex::new(Vec::new()),
        }
    }

    fn call_word(&self) -> &'static str {
        let Ok(results) = self.results.lock() else {
            return "Calls";
        };
        let mut grpc = false;
        let mut http = false;
        for result in results.iter() {
            if result.family == "httf" {
                http = true;
            } else {
                grpc = true;
            }
        }
        match (grpc, http) {
            (true, false) => "gRPC",
            (false, true) => "HTTP",
            _ => "Calls",
        }
    }

    #[expect(clippy::too_many_arguments)]
    pub fn render_summary(
        &self,
        total: usize,
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_ms: u64,
        errors: &[String],
        metrics: &apif_state::ExecutionMetrics,
    ) -> String {
        let dim = style::dim_style();
        let brand = format!("{} ", style::bold_style().apply_to("grpctestify"));
        let heavy = format!("{brand}{}", dim.apply_to("═".repeat(68)));
        let light = dim.apply_to("─".repeat(80)).to_string();

        let avg = if total > 0 {
            metrics.sum_test_ms as f64 / total as f64
        } else {
            0.0
        };

        let mut o = String::new();
        let _ = writeln!(o);
        let _ = writeln!(o, "{heavy}");
        let dur = style::format_duration_ms(duration_ms);
        if failed > 0 {
            let _ = writeln!(
                o,
                "   {}  {} failed · {} passed · {}",
                style::fail_style().apply_to("✗ FAILED"),
                style::fail_style().apply_to(failed),
                passed,
                dim.apply_to(dur)
            );
        } else {
            let _ = writeln!(
                o,
                "   {}  {} passed · {}",
                style::pass_style().apply_to("✓ PASSED"),
                style::pass_style().apply_to(passed),
                dim.apply_to(dur)
            );
        }
        let _ = writeln!(o, "{light}");

        let _ = writeln!(o, "📊 Execution Statistics:");
        let _ = writeln!(o, "   • Total tests: {}", total);
        let _ = writeln!(o, "   • Passed: {}", style::pass_style().apply_to(passed));
        let _ = writeln!(
            o,
            "   • Failed: {}",
            if failed > 0 {
                style::fail_style().apply_to(failed).to_string()
            } else {
                failed.to_string()
            }
        );
        let _ = writeln!(o, "   • Skipped: {}", skipped);
        let _ = writeln!(
            o,
            "   • Duration: {}",
            style::format_duration_ms(duration_ms)
        );
        let _ = writeln!(o, "   • Average per test: {:.0}ms", avg);
        if metrics.rpc_calls > 0 {
            let avg_rpc = metrics.total_rpc_ms as f64 / metrics.rpc_calls as f64;
            let _ = writeln!(
                o,
                "   • {}: total {}ms, avg {:.0}ms per call",
                self.call_word(),
                metrics.total_rpc_ms,
                avg_rpc
            );
        }
        let overhead = metrics.sum_test_ms.saturating_sub(metrics.total_rpc_ms);
        let avg_overhead = if total > 0 {
            overhead as f64 / total as f64
        } else {
            0.0
        };
        let _ = writeln!(
            o,
            "   • Overhead: {}ms total, avg {:.0}ms per test",
            overhead, avg_overhead
        );
        if total > 1 && self.env_info.parallel_jobs > 1 {
            let _ = writeln!(
                o,
                "   • Mode: Parallel ({} workers)",
                self.env_info.parallel_jobs
            );
        } else {
            let _ = writeln!(o, "   • Mode: Sequential");
        }
        let executed = passed + failed;
        if self.env_info.dry_run {
            let _ = writeln!(o, "   • Success rate: N/A (dry-run mode)");
        } else if executed > 0 {
            let _ = writeln!(
                o,
                "   • Success rate: {:.0}% ({}/{} executed)",
                (passed as f64 / executed as f64) * 100.0,
                passed,
                executed
            );
        } else {
            let _ = writeln!(o, "   • Success rate: N/A (no tests executed)");
        }
        let _ = writeln!(o, "{light}");

        if !errors.is_empty() {
            let _ = writeln!(o, "{}", style::fail_style().apply_to("❌ Failed Tests:"));
            for error in errors {
                let _ = writeln!(o, "   • {}", error);
            }
            let _ = writeln!(o, "{light}");
        }

        let _ = writeln!(o, "🔧 Environment:");
        let _ = writeln!(o, "   • Address: {}", self.env_info.address);
        let _ = writeln!(o, "   • Sort Mode: {}", self.env_info.sort_mode);
        let _ = writeln!(
            o,
            "   • Dry Run: {}",
            if self.env_info.dry_run {
                "Enabled"
            } else {
                "Disabled (real calls)"
            }
        );
        if self.env_info.warnings.is_empty() {
            let _ = writeln!(o, "✨ No warnings detected");
        } else {
            let _ = writeln!(
                o,
                "⚠️  {} warning{}:",
                self.env_info.warnings.len(),
                if self.env_info.warnings.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
            for warning in &self.env_info.warnings {
                let _ = writeln!(o, "   • {warning}");
            }
        }
        let _ = writeln!(o, "{}", dim.apply_to("═".repeat(80)));
        o
    }

    pub fn render_verbose(&self, results: &[TestResult]) -> String {
        const NAME_WIDTH: usize = 52;
        let dim = style::dim_style();
        let mut o = String::new();

        for r in results {
            let file = r.meta.name.as_deref().unwrap_or(&r.name);
            let styled = match r.status {
                TestStatus::Pass => style::pass_style().apply_to("PASS").to_string(),
                TestStatus::Fail => style::fail_style().apply_to("FAIL").to_string(),
                TestStatus::Skip => style::skip_style().apply_to("SKIP").to_string(),
            };
            let _ = writeln!(o);
            let _ = writeln!(
                o,
                "   {}  {}  {}",
                styled,
                file,
                dim.apply_to(style::format_duration_ms(r.duration_ms))
            );

            if r.assertions.is_empty() {
                let (icon, text) = match r.status {
                    TestStatus::Pass => (
                        style::pass_icon().to_string(),
                        "RESPONSE matched".to_string(),
                    ),
                    TestStatus::Skip => (style::skip_icon().to_string(), "skipped".to_string()),
                    TestStatus::Fail => (
                        style::fail_icon().to_string(),
                        r.error_message.as_deref().unwrap_or("failed").to_string(),
                    ),
                };
                let mut lines = text.lines();
                if let Some(first) = lines.next() {
                    let _ = writeln!(o, "  {icon} {first}");
                }
                for extra in lines {
                    let _ = writeln!(o, "       {}", dim.apply_to(extra.trim()));
                }
                continue;
            }

            let mut last_endpoint: Option<&str> = None;
            let mut showed_response = false;
            for a in &r.assertions {
                if let Some(ep) = a.endpoint.as_deref()
                    && last_endpoint != Some(ep)
                {
                    let _ = writeln!(o, "   {}", dim.apply_to(format!("↳ {ep}")));
                    last_endpoint = Some(ep);
                }
                let icon = if a.passed {
                    style::pass_icon().to_string()
                } else {
                    style::fail_icon().to_string()
                };
                let dur = dim.apply_to(style::format_duration_ms(a.elapsed_ms));
                let pad = NAME_WIDTH.saturating_sub(a.expression.chars().count());
                let loc = dim.apply_to(format!("line {}", a.line));
                let _ = writeln!(
                    o,
                    "  {icon} {}{}  {loc}  {dur}",
                    a.expression,
                    " ".repeat(pad)
                );

                if a.passed {
                    continue;
                }
                match (&a.expected, &a.actual) {
                    (Some(exp), Some(act)) => {
                        let _ = writeln!(
                            o,
                            "       {} {}",
                            dim.apply_to("expected"),
                            style::pass_style().apply_to(exp)
                        );
                        let _ = writeln!(
                            o,
                            "       {} {}",
                            dim.apply_to("actual  "),
                            style::fail_style().apply_to(act)
                        );
                    }
                    _ => {
                        if let Some(msg) = &a.message {
                            for l in msg.lines() {
                                let _ = writeln!(o, "       {}", dim.apply_to(l.trim()));
                            }
                        }
                    }
                }
                if !showed_response
                    && let Some(ex) = &r.exchange
                    && !ex.response.is_empty()
                {
                    let joined = ex
                        .response
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let shown = if joined.chars().count() > 200 {
                        format!("{}…", joined.chars().take(200).collect::<String>())
                    } else {
                        joined
                    };
                    let _ = writeln!(o, "       {} {}", dim.apply_to("response"), shown);
                    showed_response = true;
                }
            }
        }
        o
    }

    pub fn render_slowest_tests(
        &self,
        test_results: &[apif_state::TestResult],
        limit: usize,
    ) -> String {
        if !matches!(self.mode, ConsoleMode::Verbose) || test_results.is_empty() {
            return String::new();
        }
        let mut sorted = test_results.to_vec();
        sorted.sort_by_key(|b| Reverse(b.duration_ms));

        let mut o = String::new();
        let _ = writeln!(o, "🐢 Slowest Tests:");
        for (i, result) in sorted.iter().take(limit).enumerate() {
            let name = result.meta.name.as_deref().unwrap_or(&result.name);
            let _ = writeln!(
                o,
                "   {}. {name} ({})",
                i + 1,
                style::duration_flagged(result.duration_ms, SLOW_TEST_THRESHOLD_MS)
            );
        }
        let _ = writeln!(o);
        o
    }

    pub fn render_slowest_assertions(
        &self,
        test_results: &[apif_state::TestResult],
        limit: usize,
    ) -> String {
        if !matches!(self.mode, ConsoleMode::Verbose) {
            return String::new();
        }
        let mut all: Vec<(&str, &apif_state::AssertionRecord)> = test_results
            .iter()
            .flat_map(|r| r.assertions.iter().map(move |a| (r.name.as_str(), a)))
            .collect();
        if all.is_empty() {
            return String::new();
        }
        all.sort_by_key(|(_, a)| Reverse(a.elapsed_ms));

        let mut o = String::new();
        let _ = writeln!(o, "⏱ Slowest Assertions:");
        for (i, (test_name, record)) in all.iter().take(limit).enumerate() {
            let _ = writeln!(
                o,
                "   {}. {test_name} (line {}, {}): {}",
                i + 1,
                record.line,
                style::duration_flagged(record.elapsed_ms, SLOW_TEST_THRESHOLD_MS),
                record.expression
            );
        }
        let _ = writeln!(o);
        o
    }
}

impl super::Reporter for ConsoleReporter {
    fn on_test_start(&self, _test_name: &str) {}

    fn on_test_end(&self, _test_name: &str, result: &TestResult) {
        if let Ok(mut results) = self.results.lock() {
            results.push(result.clone());
        }

        if matches!(self.mode, ConsoleMode::Dots) {
            let ch = match result.status {
                TestStatus::Pass => ".",
                TestStatus::Fail => "E",
                TestStatus::Skip => "S",
            };
            let _guard = self.dots_lock.lock().unwrap_or_else(|e| e.into_inner());
            print!("{ch}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let count = self.dots_count.fetch_add(1, Ordering::Relaxed) + 1;
            if count >= 80 {
                println!();
                self.dots_count.store(0, Ordering::Relaxed);
            }
        }
    }

    fn on_suite_end(&self, results: &apif_state::TestResults) -> anyhow::Result<()> {
        if matches!(self.mode, ConsoleMode::Dots) && self.dots_count.load(Ordering::Relaxed) > 0 {
            println!();
        }

        let results_guard = results.all();
        let total = results_guard.len();
        let passed = results_guard
            .iter()
            .filter(|r| r.status == TestStatus::Pass)
            .count();
        let failed = results_guard
            .iter()
            .filter(|r| r.status == TestStatus::Fail)
            .count();
        let skipped = results_guard
            .iter()
            .filter(|r| r.status == TestStatus::Skip)
            .count();

        let mut errors = Vec::new();
        for result in results_guard {
            if result.status == TestStatus::Fail {
                let display_name = result.meta.name.as_ref().unwrap_or(&result.name);
                let mut error_line = format!("{} ({}ms)", display_name, result.duration_ms);
                if let Some(ref error_msg) = result.error_message {
                    error_line.push_str(&format!("\n      Error: {}", error_msg));
                }
                if !result.meta.tags.is_empty() {
                    error_line.push_str(&format!(" [{}]", result.meta.tags.join(", ")));
                }
                errors.push(error_line);
            }
        }

        let metrics = &results.metrics;
        let mut out = String::new();
        if matches!(self.mode, ConsoleMode::Verbose) {
            out.push_str(&self.render_verbose(results_guard));
        }
        out.push_str(&self.render_summary(
            total,
            passed,
            failed,
            skipped,
            metrics.total_duration_ms,
            &errors,
            metrics,
        ));
        out.push_str(&self.render_slowest_tests(results_guard, 5));
        out.push_str(&self.render_slowest_assertions(results_guard, 5));
        print!("{out}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reporter;
    use apif_state::{AssertionRecord, TestResult};
    use std::assert_matches;

    fn env_info() -> EnvironmentInfo {
        EnvironmentInfo {
            address: "localhost:8080".into(),
            parallel_jobs: 4,
            sort_mode: "name".into(),
            dry_run: false,
            warnings: Vec::new(),
        }
    }

    fn record(expr: &str, passed: bool, ms: u64) -> AssertionRecord {
        AssertionRecord {
            line: 1,
            expression: expr.into(),
            passed,
            elapsed_ms: ms,
            message: if passed {
                None
            } else {
                Some("expected X got Y".into())
            },
            endpoint: None,
            expected: (!passed).then(|| "\"active\"".into()),
            actual: (!passed).then(|| "\"pending\"".into()),
            hint: None,
        }
    }

    #[test]
    fn console_mode_debug() {
        assert_eq!(format!("{:?}", ConsoleMode::Dots), "Dots");
        assert_eq!(format!("{:?}", ConsoleMode::Verbose), "Verbose");
        assert_eq!(format!("{:?}", ConsoleMode::Silent), "Silent");
    }

    #[test]
    fn console_reporter_new() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 10, env_info());
        assert_matches!(reporter.mode, ConsoleMode::Silent);
    }

    #[test]
    fn render_summary_pass_contains_verdict_and_stats() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let metrics = apif_state::ExecutionMetrics::default();
        let out = reporter.render_summary(3, 3, 0, 0, 120, &[], &metrics);
        assert!(out.contains("grpctestify"), "brand header: {out}");
        assert!(out.contains("✓ PASSED"));
        assert!(out.contains("3 passed"));
        assert!(out.contains("Total tests: 3"));
        assert!(out.contains("Mode: Parallel (4 workers)"));
    }

    #[test]
    fn the_call_line_names_the_wire_the_run_dialled() {
        let metrics = apif_state::ExecutionMetrics {
            rpc_calls: 2,
            total_rpc_ms: 8,
            sum_test_ms: 20,
            ..Default::default()
        };
        let ended = |reporter: &ConsoleReporter, families: &[&str]| {
            for (i, family) in families.iter().enumerate() {
                let mut result = TestResult::pass(format!("t{i}"), 4, Some(4));
                result.family = (*family).to_string();
                reporter.on_test_end("t", &result);
            }
            reporter.render_summary(families.len(), families.len(), 0, 0, 10, &[], &metrics)
        };

        let http = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let said = ended(&http, &["httf", "httf"]);
        assert!(said.contains("• HTTP: total"), "{said}");

        let grpc = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let said = ended(&grpc, &["gctf", "gctf"]);
        assert!(said.contains("• gRPC: total"), "{said}");

        let both = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let said = ended(&both, &["gctf", "httf"]);
        assert!(said.contains("• Calls: total"), "{said}");
    }

    #[test]
    fn render_summary_fail_contains_failed_block() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let metrics = apif_state::ExecutionMetrics::default();
        let out = reporter.render_summary(2, 1, 1, 0, 50, &["bad.gctf (5ms)".into()], &metrics);
        assert!(out.contains("✗ FAILED"));
        assert!(out.contains("1 failed"));
        assert!(out.contains("Failed Tests:"));
        assert!(out.contains("bad.gctf"));
    }

    #[test]
    fn render_summary_single_test_is_sequential() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let metrics = apif_state::ExecutionMetrics::default();
        let out = reporter.render_summary(1, 1, 0, 0, 10, &[], &metrics);
        assert!(out.contains("Mode: Sequential"));
    }

    #[test]
    fn render_verbose_groups_by_file_with_assertions() {
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let results = vec![
            TestResult::pass("tests/a.gctf", 12, Some(4)).with_assertions(vec![
                record(".id == 1", true, 2),
                record("@uuid(.x)", true, 1),
            ]),
            TestResult::fail("tests/b.gctf", "mismatch".into(), 15, Some(3))
                .with_assertions(vec![record(".status == \"ok\"", false, 3)]),
        ];
        let out = reporter.render_verbose(&results);
        assert!(out.contains("PASS"));
        assert!(out.contains("tests/a.gctf"));
        assert!(out.contains(".id == 1"));
        assert!(out.contains("@uuid(.x)"));
        assert!(out.contains("FAIL"));
        assert!(out.contains(".status == \"ok\""));
        assert!(out.contains("expected"));
        assert!(out.contains("\"active\""));
        assert!(out.contains("actual"));
        assert!(out.contains("\"pending\""));
        assert!(out.contains("line 1"));
    }

    #[test]
    fn render_verbose_fail_shows_server_response() {
        use apif_state::CapturedExchange;
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let exchange = CapturedExchange::capture(
            Default::default(),
            Default::default(),
            vec![serde_json::json!({"status": "pending"})],
        );
        let result = TestResult::fail("tests/b.gctf", "mismatch".into(), 15, Some(3))
            .with_assertions(vec![record(".status == \"active\"", false, 3)])
            .with_exchange(Some(exchange));
        let out = reporter.render_verbose(&[result]);
        assert!(out.contains("response"));
        assert!(out.contains("\"status\":\"pending\""));
    }

    #[test]
    fn render_verbose_transport_error_shows_full_message() {
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let result = TestResult::fail(
            "tests/down.gctf",
            "gRPC error code=14 message=connection refused".into(),
            2,
            None,
        );
        let out = reporter.render_verbose(&[result]);
        assert!(out.contains("connection refused"));
    }

    #[test]
    fn render_verbose_no_asserts_shows_synthetic_line() {
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let results = vec![TestResult::pass("tests/plain.gctf", 8, Some(8))];
        let out = reporter.render_verbose(&results);
        assert!(out.contains("tests/plain.gctf"));
        assert!(out.contains("RESPONSE matched"));
    }

    #[test]
    fn render_verbose_groups_by_endpoint_for_multidoc() {
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let mut a = record(".token != null", true, 2);
        a.endpoint = Some("auth.Auth/Login".into());
        let mut b = record(".id > 0", true, 1);
        b.endpoint = Some("user.User/Create".into());
        let results =
            vec![TestResult::pass("tests/flow.gctf", 20, Some(6)).with_assertions(vec![a, b])];
        let out = reporter.render_verbose(&results);
        assert!(out.contains("↳ auth.Auth/Login"));
        assert!(out.contains("↳ user.User/Create"));
    }

    #[test]
    fn render_slowest_assertions_empty_when_no_assertions() {
        let reporter = ConsoleReporter::new(ConsoleMode::Verbose, 0, env_info());
        let results = vec![TestResult::pass("a.gctf", 5, None)];
        assert!(reporter.render_slowest_assertions(&results, 5).is_empty());
    }

    #[test]
    fn render_slowest_tests_empty_when_not_verbose() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 0, env_info());
        let results = vec![TestResult::pass("a.gctf", 5, None)];
        assert!(reporter.render_slowest_tests(&results, 5).is_empty());
    }

    #[test]
    fn console_reporter_lifecycle_silent() {
        let reporter = ConsoleReporter::new(ConsoleMode::Silent, 2, env_info());
        reporter.on_test_start("test1");
        reporter.on_test_end("test1", &TestResult::pass("test1.gctf", 100, Some(50)));
        reporter.on_test_end(
            "test2",
            &TestResult::fail("test2.gctf", "error".into(), 200, Some(100)),
        );
        let results = apif_state::TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }

    #[test]
    fn console_reporter_empty_results() {
        let reporter = ConsoleReporter::new(ConsoleMode::Dots, 0, env_info());
        let results = apif_state::TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }
}
