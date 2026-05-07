//! Output reporters for test results.
//!
//! Each reporter maintains its own state and is responsible for formatting its output.

pub mod allure;
pub mod bench;
pub mod console;
pub mod coverage;
pub mod diagnostics;
pub mod json;
pub mod junit;
pub mod kernel;
pub mod streaming;

use crate::state::{TestResult, TestResults};
pub use allure::AllureReporter;
use anyhow::Result;
pub use console::ConsoleReporter;
pub use coverage::CoverageCollector;
pub use diagnostics::{
    AstOverview, BenchResolvedOption, CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity,
    InspectReport, SectionInfo,
};
pub use json::JsonReporter;
pub use junit::JunitReporter;
pub use streaming::StreamingJsonReporter;

/// Reporter that accumulates test results and produces formatted output.
/// Each reporter is responsible for its own state management and formatting logic.
pub trait Reporter: Send + Sync {
    /// Called when a test starts
    fn on_test_start(&self, _test_name: &str) {}

    /// Called when a test finishes - reporter accumulates the result
    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {}

    /// Called when the entire suite finishes - produces the final report.
    /// Reporters should use their accumulated state to format output.
    fn on_suite_end(&self, results: &TestResults) -> Result<()>;
}
