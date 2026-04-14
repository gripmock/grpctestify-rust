//! Output reporters for test results.
//!
//! This module provides multiple output formats for test execution results:
//!
//! | Reporter | Purpose |
//! |----------|---------|
//! | [`ConsoleReporter`] | Pytest-style terminal output with progress bars |
//! | [`JsonReporter`] | Machine-readable JSON results |
//! | [`JunitReporter`] | JUnit XML for CI/CD integration |
//! | [`AllureReporter`] | Allure TestOps compatible reports |
//! | [`StreamingJsonReporter`] | NDJSON stream for real-time consumption |
//! | [`CoverageCollector`] | gRPC method and protobuf field coverage |

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
    fn on_test_start(&self, _test_name: &str) {}

    /// Called when a test finishes
    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {}

    /// Called when the entire suite finishes
    fn on_suite_end(&self, results: &TestResults) -> Result<()>;
}
