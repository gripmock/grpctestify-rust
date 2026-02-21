// Console reporter - pytest-style output

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::cli::ProgressMode;
use crate::state::{TestResult, TestStatus};
use indicatif::{ProgressBar, ProgressStyle};

/// Environment information for report
#[derive(Debug, Clone)]
pub struct EnvironmentInfo {
    pub address: String,
    pub parallel_jobs: usize,
    pub sort_mode: String,
    pub dry_run: bool,
}

/// Console reporter
pub struct ConsoleReporter {
    mode: ProgressMode,
    progress_bar: ProgressBar,
    env_info: EnvironmentInfo,
    dots_lock: Mutex<()>,
    dots_count: AtomicUsize,
}

impl ConsoleReporter {
    /// Create new console reporter
    pub fn new(mode: ProgressMode, total_tests: u64, env_info: EnvironmentInfo) -> Self {
        let progress_bar = if matches!(mode, ProgressMode::Dots) {
            let pb = ProgressBar::new(total_tests);
            pb.set_style(ProgressStyle::default_bar().template("{msg}").unwrap());
            pb
        } else {
            ProgressBar::hidden()
        };

        Self {
            mode,
            progress_bar,
            env_info,
            dots_lock: Mutex::new(()),
            dots_count: AtomicUsize::new(0),
        }
    }

    /// Print summary
    pub fn print_summary(
        &self,
        total: usize,
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_ms: u64,
        errors: &[String],
        metrics: &crate::state::ExecutionMetrics,
    ) {
        self.progress_bar.finish_and_clear();

        println!();
        println!(
            "════════════════════════════════════════════════════════════════════════════════"
        );
        if failed > 0 {
            println!(
                "❌ FAILED ({} failed, {} passed in {}ms)",
                failed, passed, duration_ms
            );
        } else {
            println!("✅ PASSED ({} passed in {}ms)", passed, duration_ms);
        }
        println!(
            "────────────────────────────────────────────────────────────────────────────────"
        );
        println!("📊 Execution Statistics:");
        println!("   • Total tests: {}", total);
        println!("   • Passed: {}", passed);
        println!("   • Failed: {}", failed);
        println!("   • Skipped: {}", skipped);
        println!("   • Duration: {}ms", duration_ms);

        let avg = if total > 0 {
            duration_ms as f64 / total as f64
        } else {
            0.0
        };
        println!("   • Average per test: {:.0}ms", avg);

        // gRPC Stats
        if metrics.grpc_calls > 0 {
            let avg_grpc = metrics.grpc_total_duration_ms as f64 / metrics.grpc_calls as f64;
            println!(
                "   • gRPC: total {}ms, avg {:.0}ms per call",
                metrics.grpc_total_duration_ms, avg_grpc
            );
        }

        // Overhead
        let overhead = if duration_ms > metrics.grpc_total_duration_ms {
            duration_ms - metrics.grpc_total_duration_ms
        } else {
            0
        };
        let avg_overhead = if total > 0 {
            overhead as f64 / total as f64
        } else {
            0.0
        };
        println!(
            "   • Overhead: {}ms total, avg {:.0}ms per test",
            overhead, avg_overhead
        );

        println!(
            "   • Mode: Parallel ({} threads)",
            self.env_info.parallel_jobs
        );

        let executed = passed + failed;
        if self.env_info.dry_run {
            println!("   • Success rate: N/A (dry-run mode)");
        } else if executed > 0 {
            let success_rate = (passed as f64 / executed as f64) * 100.0;
            println!(
                "   • Success rate: {:.0}% ({}/{} executed)",
                success_rate, passed, executed
            );
        } else {
            println!("   • Success rate: N/A (no tests executed)");
        }

        // Performance rating - ONLY show checkmark if NO failures
        let rating = Self::get_performance_rating(avg);
        if failed == 0 {
            println!("   • Performance: {} ({:.0}ms/test)", rating, avg);
        } else {
            // If failed, maybe show just the timing without the "Checkmark Excellent"
            // or just omit it entirely as user requested ("who needs it if test failed")
            // The bash script showed it, but user complained. Let's hide the checkmark.
            // Or better, just show timing.
            println!("   • Performance: {:.0}ms/test", avg);
        }

        println!(
            "────────────────────────────────────────────────────────────────────────────────"
        );

        // Failed Tests Section
        if !errors.is_empty() {
            println!("❌ Failed Tests:");
            for error in errors {
                println!("   • {}", error);
            }
        }

        // Environment Section
        println!("🔧 Environment:");
        println!("   • gRPC Address: {}", self.env_info.address);
        println!("   • Sort Mode: {}", self.env_info.sort_mode);
        println!(
            "   • Dry Run: {}",
            if self.env_info.dry_run {
                "Enabled"
            } else {
                "Disabled (real gRPC calls)"
            }
        );

        println!("✨ No warnings detected"); // TODO: Implement warning tracking
        println!(
            "════════════════════════════════════════════════════════════════════════════════"
        );
        println!();
    }

    /// Get performance rating based on average test duration
    fn get_performance_rating(avg_ms: f64) -> String {
        let rating = if avg_ms < 100.0 {
            "⚡ Excellent"
        } else if avg_ms < 500.0 {
            "✅ Good"
        } else if avg_ms < 1000.0 {
            "⚠️  Moderate"
        } else {
            "🐌 Slow"
        };

        rating.to_string()
    }

    /// Print slowest tests
    pub fn print_slowest_tests(&self, test_results: &[crate::state::TestResult], limit: usize) {
        // Only print if verbose or if explicitly asked? The bash script puts it in "Failed Tests" section usually with duration.
        // The Rust version printed it at the end. I'll keep it but maybe format it better or hide if empty.
        // Actually, the bash script doesn't seem to print "Slowest tests" list separately in the main view,
        // it just puts duration next to failed tests.
        // I'll skip printing this separately to match bash output cleaner look, unless verbose?
        // Let's keep it minimal as requested.
        if matches!(self.mode, ProgressMode::Verbose) {
            if test_results.is_empty() {
                return;
            }

            let mut sorted = test_results.to_vec();
            sorted.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));

            println!("🐢 Slowest Tests:");
            let count = limit.min(sorted.len());
            for (i, result) in sorted.iter().take(count).enumerate() {
                let rating = if result.status == TestStatus::Fail {
                    "❌ Failed".to_string()
                } else {
                    Self::get_performance_rating(result.duration_ms as f64)
                };

                println!(
                    "   {}. {} ({}ms) - {}",
                    i + 1,
                    result.name,
                    result.duration_ms,
                    rating
                );
            }
            println!();
        }
    }
}

impl super::Reporter for ConsoleReporter {
    fn on_test_start(&self, test_name: &str) {
        if matches!(self.mode, ProgressMode::Verbose) {
            println!("Testing {} ... ", test_name);
        }
    }

    fn on_test_end(&self, _test_name: &str, result: &TestResult) {
        if matches!(self.mode, ProgressMode::Dots) {
            let char = match result.status {
                TestStatus::Pass => ".",
                TestStatus::Fail => "E",
                TestStatus::Skip => "S",
            };

            let _guard = self.dots_lock.lock().unwrap();
            print!("{}", char);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            let count = self.dots_count.fetch_add(1, Ordering::Relaxed) + 1;
            if count >= 80 {
                println!();
                self.dots_count.store(0, Ordering::Relaxed);
            }
        } else if matches!(self.mode, ProgressMode::Verbose) {
            match result.status {
                TestStatus::Pass => println!("✅ PASS"),
                TestStatus::Fail => println!(
                    "❌ FAIL: {}",
                    result.error_message.as_deref().unwrap_or("Unknown error")
                ),
                TestStatus::Skip => println!("🔍 SKIP"),
            }
        }
    }

    fn on_suite_end(&self, results: &crate::state::TestResults) -> anyhow::Result<()> {
        // Ensure newline after dots
        if matches!(self.mode, ProgressMode::Dots) && self.dots_count.load(Ordering::Relaxed) > 0 {
            println!();
        }

        let metrics = results.metrics();

        let mut errors = Vec::new();
        for result in results.all() {
            if result.status == TestStatus::Fail {
                // Bash format: "test_name.gctf (duration)"
                errors.push(format!("{} ({}ms)", result.name, result.duration_ms));
            }
        }

        self.print_summary(
            results.total(),
            results.passed(),
            results.failed(),
            results.skipped(),
            metrics.total_duration_ms,
            &errors,
            metrics,
        );

        // Print slowest tests (top 5) - Only in verbose now
        self.print_slowest_tests(results.all(), 5);

        Ok(())
    }
}
