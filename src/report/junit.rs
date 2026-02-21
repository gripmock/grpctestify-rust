// JUnit reporter - outputs test results in JUnit XML format

use super::Reporter;
use crate::state::{TestResult, TestResults, TestStatus};
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

/// JUnit reporter
pub struct JunitReporter {
    output_path: PathBuf,
}

impl JunitReporter {
    /// Create new JUnit reporter
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

impl Reporter for JunitReporter {
    fn on_test_start(&self, _test_name: &str) {
        // No-op for JUnit file reporter
    }

    fn on_test_end(&self, _test_name: &str, _result: &TestResult) {
        // No-op for standard JUnit report
    }

    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let metrics = results.metrics();

        // Basic XML construction (avoiding external XML deps for simplicity if possible)
        // If complex, we might want to pull in 'xml-rs' or similar.
        // For now, simple string construction should suffice for JUnit format.

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuites name=\"grpctestify\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\">\n",
            metrics.total_duration_ms as f64 / 1000.0,
            results.total(),
            results.failed(),
            results.skipped()
        ));

        xml.push_str(&format!(
            "  <testsuite name=\"e2e\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\">\n",
            metrics.total_duration_ms as f64 / 1000.0,
            results.total(),
            results.failed(),
            results.skipped()
        ));

        for result in results.all() {
            let classname = "grpctestify.e2e"; // Group all under e2e for now or derive from path?
                                               // Extract file name from full path for test name
            let name = std::path::Path::new(&result.name)
                .file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_else(|| result.name.clone().into());

            xml.push_str(&format!(
                "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\">\n",
                name,
                classname,
                result.duration_ms as f64 / 1000.0
            ));

            match result.status {
                TestStatus::Fail => {
                    let msg = result.error_message.as_deref().unwrap_or("Test failed");
                    // Simple XML escaping
                    let escaped_msg = msg
                        .replace("&", "&amp;")
                        .replace("<", "&lt;")
                        .replace(">", "&gt;")
                        .replace("\"", "&quot;")
                        .replace("'", "&apos;");

                    xml.push_str(&format!(
                        "      <failure message=\"{}\" type=\"AssertionError\">{}</failure>\n",
                        escaped_msg, escaped_msg
                    ));
                }
                TestStatus::Skip => {
                    let msg = result.error_message.as_deref().unwrap_or("Test skipped");
                    let escaped_msg = msg
                        .replace("&", "&amp;")
                        .replace("<", "&lt;")
                        .replace(">", "&gt;")
                        .replace("\"", "&quot;")
                        .replace("'", "&apos;");

                    xml.push_str(&format!("      <skipped message=\"{}\" />\n", escaped_msg));
                }
                TestStatus::Pass => {}
            }

            xml.push_str("    </testcase>\n");
        }

        xml.push_str("  </testsuite>\n");
        xml.push_str("</testsuites>\n");

        let mut file = File::create(&self.output_path).with_context(|| {
            format!(
                "Failed to create JUnit report file: {}",
                self.output_path.display()
            )
        })?;

        file.write_all(xml.as_bytes())
            .context("Failed to write JUnit XML content")?;

        Ok(())
    }
}
