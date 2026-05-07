// JSON reporter - outputs test results to a JSON file

use super::Reporter;
use crate::state::TestResults;
use anyhow::{Context, Result};
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
            report_context: {
                let ctx = crate::report::kernel::report_context();
                JsonReportContext {
                    tool: ctx.tool,
                    version: ctx.version,
                    generated_at: ctx.generated_at,
                }
            },
        };

        serde_json::to_writer_pretty(file, &report)
            .context("Failed to serialize test results to JSON")?;

        Ok(())
    }
}
