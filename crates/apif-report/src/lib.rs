//! Output reporters for test results — protocol-agnostic.
//! gRPC-specific reporters (allure, coverage, kernel) stay in the main project.

pub mod console;
pub mod diagnostics;
pub mod html;
pub mod json;
pub mod junit;
pub mod streaming;
pub mod style;
pub mod yaml;

use anyhow::Result;
use apif_state::{TestResult, TestResults};
pub use console::{ConsoleMode, ConsoleReporter};
pub use diagnostics::{
    AstOverview, BenchResolvedOption, CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity,
    InspectReport, SectionInfo,
};
pub use html::HtmlReporter;
pub use json::JsonReporter;
pub use junit::JunitReporter;
pub use streaming::StreamingJsonReporter;
pub use yaml::YamlReporter;

/// Each reporter accumulates results via `on_test_end`, then formats and
/// writes its output when `on_suite_end` flushes.
pub trait Reporter: Send + Sync {
    fn on_test_start(&self, _test_name: &str) {}

    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {}

    fn on_suite_end(&self, results: &TestResults) -> Result<()>;
}
