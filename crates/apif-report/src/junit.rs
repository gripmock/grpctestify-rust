// JUnit reporter - outputs test results in JUnit XML format

use super::Reporter;
use anyhow::{Context, Result};
use apif_state::{TestResults, TestStatus};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn escape_xml(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}

struct TestCaseBuilder {
    name: String,
    classname: String,
    duration_ms: u64,
    status: TestStatus,
    error_message: Option<String>,
    tags: Vec<String>,
    extra_properties: Vec<(String, String)>,
}

impl TestCaseBuilder {
    fn to_xml(&self) -> String {
        let escaped_name = escape_xml(&self.name);
        let escaped_classname = escape_xml(&self.classname);
        let duration = self.duration_ms as f64 / 1000.0;

        let mut xml = format!(
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
            escaped_name, escaped_classname, duration
        );

        if self.tags.is_empty() && self.extra_properties.is_empty() {
            xml.push_str(">\n");
        } else {
            xml.push_str(">\n      <properties>\n");
            for tag in &self.tags {
                xml.push_str(&format!(
                    "        <property name=\"tag\" value=\"{}\"/>\n",
                    escape_xml(tag)
                ));
            }
            for (key, value) in &self.extra_properties {
                xml.push_str(&format!(
                    "        <property name=\"{}\" value=\"{}\"/>\n",
                    escape_xml(key),
                    escape_xml(value)
                ));
            }
            xml.push_str("      </properties>\n");
        }

        match self.status {
            TestStatus::Fail => {
                let msg = self.error_message.as_deref().unwrap_or("Test failed");
                xml.push_str(&format!(
                    "      <failure message=\"{}\" type=\"AssertionError\">{}</failure>\n",
                    escape_xml(msg),
                    escape_xml(msg)
                ));
            }
            TestStatus::Skip => {
                let msg = self.error_message.as_deref().unwrap_or("Test skipped");
                xml.push_str(&format!(
                    "      <skipped message=\"{}\" />\n",
                    escape_xml(msg)
                ));
            }
            TestStatus::Pass => {}
        }

        xml.push_str("    </testcase>\n");
        xml
    }
}

/// JUnit reporter
pub struct JunitReporter {
    output_path: PathBuf,
}

impl JunitReporter {
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

impl Reporter for JunitReporter {
    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let metrics = results.metrics();
        let duration = metrics.total_duration_ms as f64 / 1000.0;
        let total = results.total();
        let failures = results.failed();
        let skipped = results.skipped();

        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <testsuites name=\"grpctestify\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\">\n\
             <testsuite name=\"e2e\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\">\n",
            duration, total, failures, skipped, duration, total, failures, skipped
        );

        for result in results.all() {
            let display_name = result.meta.name.as_deref().unwrap_or(&result.name);
            let classname = std::path::Path::new(&result.name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".to_string());

            let tc = TestCaseBuilder {
                name: display_name.to_string(),
                classname,
                duration_ms: result.duration_ms,
                status: result.status,
                error_message: result.error_message.clone(),
                tags: result.meta.tags.clone(),
                extra_properties: Vec::new(),
            };
            xml.push_str(&tc.to_xml());
        }

        xml.push_str("  </testsuite>\n</testsuites>\n");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reporter;
    use apif_state::TestResult;

    #[test]
    fn test_junit_reporter_new() {
        let reporter = JunitReporter::new(PathBuf::from("test.xml"));
        assert_eq!(reporter.output_path.to_str(), Some("test.xml"));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("a&b"), "a&amp;b");
        assert_eq!(escape_xml("a<b"), "a&lt;b");
        assert_eq!(escape_xml("a>b"), "a&gt;b");
        assert_eq!(escape_xml("a\"b"), "a&quot;b");
        assert_eq!(escape_xml("a'b"), "a&apos;b");
        assert_eq!(escape_xml("plain"), "plain");
    }

    #[test]
    fn test_test_case_builder_to_xml_pass() {
        let tc = TestCaseBuilder {
            name: "test1".into(),
            classname: "suite".into(),
            duration_ms: 100,
            status: TestStatus::Pass,
            error_message: None,
            tags: vec![],
            extra_properties: vec![],
        };
        let xml = tc.to_xml();
        assert!(xml.contains("testcase"), "xml: {}", xml);
        assert!(
            !xml.contains("failure"),
            "pass should not have failure: {}",
            xml
        );
        assert!(
            !xml.contains("skipped"),
            "pass should not have skipped: {}",
            xml
        );
        assert!(xml.contains("0.100"), "xml: {}", xml);
    }

    #[test]
    fn test_test_case_builder_to_xml_fail() {
        let tc = TestCaseBuilder {
            name: "test2".into(),
            classname: "suite".into(),
            duration_ms: 200,
            status: TestStatus::Fail,
            error_message: Some("assertion failed".into()),
            tags: vec![],
            extra_properties: vec![],
        };
        let xml = tc.to_xml();
        assert!(xml.contains("failure"));
        assert!(xml.contains("assertion failed"));
    }

    #[test]
    fn test_test_case_builder_to_xml_skip() {
        let tc = TestCaseBuilder {
            name: "test3".into(),
            classname: "suite".into(),
            duration_ms: 50,
            status: TestStatus::Skip,
            error_message: Some("not ready".into()),
            tags: vec![],
            extra_properties: vec![],
        };
        let xml = tc.to_xml();
        assert!(xml.contains("skipped"));
    }

    #[test]
    fn test_test_case_builder_with_tags() {
        let tc = TestCaseBuilder {
            name: "test".into(),
            classname: "suite".into(),
            duration_ms: 100,
            status: TestStatus::Pass,
            error_message: None,
            tags: vec!["api".into(), "smoke".into()],
            extra_properties: vec![("env".into(), "prod".into())],
        };
        let xml = tc.to_xml();
        assert!(xml.contains("properties"));
        assert!(xml.contains("api"));
        assert!(xml.contains("env"));
    }

    #[test]
    fn test_junit_reporter_lifecycle() {
        let reporter = JunitReporter::new(PathBuf::from("/tmp/test_junit_output.xml"));
        let mut results = TestResults::new();
        results.add(TestResult::pass("test.gctf", 100, None));
        assert!(reporter.on_suite_end(&results).is_ok());
        assert!(std::path::Path::new("/tmp/test_junit_output.xml").exists());
        let _ = std::fs::remove_file("/tmp/test_junit_output.xml");
    }

    #[test]
    fn test_junit_reporter_with_failure() {
        let reporter = JunitReporter::new(PathBuf::from("/tmp/test_junit_fail.xml"));
        let mut results = TestResults::new();
        results.add(TestResult::fail("test.gctf", "error msg".into(), 100, None));
        assert!(reporter.on_suite_end(&results).is_ok());
        let content = std::fs::read_to_string("/tmp/test_junit_fail.xml").unwrap();
        assert!(content.contains("failure"));
        assert!(content.contains("error msg"));
        let _ = std::fs::remove_file("/tmp/test_junit_fail.xml");
    }
}
