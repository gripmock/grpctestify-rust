use super::Reporter;
use crate::{tool_name, tool_version};
use anyhow::{Context, Result};
use apif_state::TestResults;
use serde::Serialize;
use std::fs::File;
use std::path::PathBuf;

pub struct JsonReporter {
    output_path: PathBuf,
}

impl JsonReporter {
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

#[derive(Serialize)]
struct JsonReportContext {
    tool: String,
    version: String,
    generated_at: i64,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    #[serde(flatten)]
    results: &'a TestResults,
    report_context: JsonReportContext,
}

impl Reporter for JsonReporter {
    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let file = File::create(&self.output_path).with_context(|| {
            format!(
                "Failed to create JSON report file: {}",
                self.output_path.display()
            )
        })?;

        let report = JsonReport {
            results,
            report_context: JsonReportContext {
                tool: tool_name().to_string(),
                version: tool_version().to_string(),
                generated_at: apif_cfg_runtime::now_timestamp(),
            },
        };

        serde_json::to_writer_pretty(file, &report)
            .context("Failed to serialize test results to JSON")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_the_tool_not_the_crate_that_formats_it() {
        crate::set_tool_identity("grpctestify", "9.9.9");
        assert_eq!(crate::tool_name(), "grpctestify");
        assert_eq!(crate::tool_version(), "9.9.9");
    }

    #[test]
    fn json_reporter_new() {
        let reporter = JsonReporter::new(PathBuf::from("test.json"));
        assert_eq!(reporter.output_path.to_str(), Some("test.json"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[cfg(not(miri))]
    fn json_reporter_lifecycle() {
        use crate::Reporter;
        use apif_state::TestResult;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_output.json");
        let reporter = JsonReporter::new(path.clone());
        reporter.on_test_start("test1");
        let pass = TestResult::pass("test1.gctf", 100, Some(50));
        reporter.on_test_end("test1", &pass);
        let results = apif_state::TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }

    #[test]
    fn json_report_serialize() {
        let results = TestResults::new();
        let ctx = JsonReportContext {
            tool: "apif".into(),
            version: "1.0".into(),
            generated_at: 1000000,
        };
        let report = JsonReport {
            results: &results,
            report_context: ctx,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"tool\":\"apif\""));
        assert!(json.contains("\"version\":\"1.0\""));
        assert!(json.contains("\"generated_at\":1000000"));
    }
}
