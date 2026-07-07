//! Output reporters for test results — protocol-agnostic.
//! gRPC-specific reporters (allure, coverage, kernel) stay in the main project.

pub mod console;
pub mod diagnostics;
pub mod json;
pub mod junit;
pub mod streaming;

use apif_state::{TestResult, TestResults};
use anyhow::Result;
pub use console::{ConsoleMode, ConsoleReporter};
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
