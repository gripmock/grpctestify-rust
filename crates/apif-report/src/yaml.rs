use super::Reporter;
use crate::{tool_name, tool_version};
use anyhow::{Context, Result};
use apif_state::TestResults;
use serde::Serialize;
use std::fs::File;
use std::path::PathBuf;

pub struct YamlReporter {
    output_path: PathBuf,
}

impl YamlReporter {
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

#[derive(Serialize)]
struct YamlReportContext {
    tool: String,
    version: String,
    generated_at: i64,
}

#[derive(Serialize)]
struct YamlReport<'a> {
    #[serde(flatten)]
    results: &'a TestResults,
    report_context: YamlReportContext,
}

impl Reporter for YamlReporter {
    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let file = File::create(&self.output_path).with_context(|| {
            format!(
                "Failed to create YAML report file: {}",
                self.output_path.display()
            )
        })?;

        let report = YamlReport {
            results,
            report_context: YamlReportContext {
                tool: tool_name().to_string(),
                version: tool_version().to_string(),
                generated_at: apif_cfg_runtime::now_timestamp(),
            },
        };

        serde_yaml_ng::to_writer(file, &report)
            .context("Failed to serialize test results to YAML")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_reporter_new() {
        let reporter = YamlReporter::new(PathBuf::from("test.yaml"));
        assert_eq!(reporter.output_path.to_str(), Some("test.yaml"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn yaml_reporter_lifecycle() {
        use crate::Reporter;
        use apif_state::TestResult;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_output.yaml");
        let reporter = YamlReporter::new(path.clone());
        reporter.on_test_start("test1");
        let pass = TestResult::pass("test1.gctf", 100, Some(50));
        reporter.on_test_end("test1", &pass);
        let results = apif_state::TestResults::new();
        assert!(reporter.on_suite_end(&results).is_ok());
    }
}
