pub mod allure;
pub mod bench;
pub mod coverage;
pub mod kernel;
pub mod rhai_reporter;

pub use apif_report::diagnostics::{
    AstOverview, BenchResolvedOption, CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity,
    DocumentStructure, InspectReport, SectionInfo,
};
pub use apif_report::{
    ConsoleMode, ConsoleReporter, HtmlReporter, JsonReporter, JunitReporter, Reporter,
    StreamingJsonReporter, YamlReporter, set_tool_identity, tool_name, tool_version,
};
pub use apif_report::{console, diagnostics, html, json, junit, streaming, style};

pub use allure::AllureReporter;
pub use coverage::CoverageCollector;
pub use rhai_reporter::{load_all_configured_reporters, load_rhai_reporters};
