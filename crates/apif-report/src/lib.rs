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
use std::sync::OnceLock;

static IDENTITY: OnceLock<(String, String)> = OnceLock::new();

pub fn set_tool_identity(name: impl Into<String>, version: impl Into<String>) {
    let _ = IDENTITY.set((name.into(), version.into()));
}

pub fn tool_name() -> &'static str {
    IDENTITY
        .get()
        .map(|(name, _)| name.as_str())
        .unwrap_or("grpctestify")
}

pub fn tool_version() -> &'static str {
    IDENTITY
        .get()
        .map(|(_, version)| version.as_str())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
}
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

pub trait Reporter: Send + Sync {
    fn on_test_start(&self, _test_name: &str) {}

    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {}

    fn on_suite_end(&self, results: &TestResults) -> Result<()>;
}
