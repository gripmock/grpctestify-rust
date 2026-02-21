// Report module - Console output and reporting

pub mod allure;
pub mod console;
pub mod coverage;
pub mod diagnostics;
pub mod json;
pub mod junit;
pub mod streaming;

use crate::state::{TestResult, TestResults};
pub use allure::AllureReporter;
use anyhow::Result;
pub use console::ConsoleReporter;
pub use coverage::CoverageCollector;
pub use diagnostics::{
    AstOverview, CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity, InspectReport,
    SectionInfo,
};
pub use json::JsonReporter;
pub use junit::JunitReporter;
pub use streaming::StreamingJsonReporter;

/// Reporter trait
pub trait Reporter: Send + Sync {
    /// Called when a test starts
    fn on_test_start(&self, test_name: &str);

    /// Called when a test finishes
    fn on_test_end(&self, test_name: &str, result: &TestResult);

    /// Called when the entire suite finishes
    fn on_suite_end(&self, results: &TestResults) -> Result<()>;
}
