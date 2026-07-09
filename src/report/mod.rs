//! Output reporters for test results.
//! Agnostic reporters live in crates/apif-report.
//! gRPC-specific reporters (allure, coverage, bench, kernel) stay local.

pub mod allure;
pub mod bench;
pub mod coverage;
pub mod kernel;

// Re-export agnostic reporters from crate
pub use apif_report::{
    ConsoleMode, ConsoleReporter, JsonReporter, JunitReporter, Reporter, StreamingJsonReporter,
};
// Re-export modules for backward compat paths like crate::report::console::EnvironmentInfo
pub use apif_report::diagnostics::{
    AstOverview, BenchResolvedOption, CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity,
    InspectReport, SectionInfo,
};
pub use apif_report::{console, diagnostics, json, junit, streaming};

pub use allure::AllureReporter;
pub use coverage::CoverageCollector;
