// JUnit reporter - outputs test results in JUnit XML format

use super::Reporter;
use anyhow::{Context, Result};
use apif_state::{TestResults, TestStatus};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Tab, LF and CR are the only control chars valid in XML 1.0.
            '\t' | '\n' | '\r' => out.push(c),
            // Strip other C0 control chars (0x00-0x08, 0x0B, 0x0C, 0x0E-0x1F):
            // they are forbidden in XML 1.0 documents — even as numeric
            // character references — and make the output unparsable by CI tools.
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

struct TestCaseBuilder {
    name: String,
    classname: String,
    duration_ms: u64,
    status: TestStatus,
    error_message: Option<String>,
    /// Detailed `<failure>` body (per-assertion expected/actual). Falls back to
    /// `error_message` when absent.
    failure_body: Option<String>,
    /// `<system-out>` body — the captured request/response exchange, when the
    /// run buffered it. `None` omits the element.
    system_out: Option<String>,
    /// `true` when the test failed before any ASSERTS ran (connection/timeout/
    /// parse) — reported as JUnit `<error>` instead of `<failure>`, which is
    /// reserved for an evaluated assertion that didn't hold.
    is_execution_error: bool,
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
            TestStatus::Fail if self.is_execution_error => {
                // No assertion was evaluated — the run failed before ASSERTS
                // (connection/timeout/parse/etc), which JUnit models as <error>.
                let msg = self.error_message.as_deref().unwrap_or("Test errored");
                xml.push_str(&format!(
                    "      <error message=\"{}\" type=\"ExecutionError\">{}</error>\n",
                    escape_xml(msg),
                    escape_xml(msg)
                ));
            }
            TestStatus::Fail => {
                let msg = self.error_message.as_deref().unwrap_or("Test failed");
                let body = self.failure_body.as_deref().unwrap_or(msg);
                xml.push_str(&format!(
                    "      <failure message=\"{}\" type=\"AssertionError\">{}</failure>\n",
                    escape_xml(msg),
                    escape_xml(body)
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

        if let Some(out) = &self.system_out {
            xml.push_str(&format!(
                "      <system-out>{}</system-out>\n",
                escape_xml(out)
            ));
        }

        xml.push_str("    </testcase>\n");
        xml
    }
}

/// Render the captured exchange (headers/trailers/response) as a `<system-out>`
/// text block. `None` when nothing was captured.
fn build_system_out(result: &apif_state::TestResult) -> Option<String> {
    let ex = result.exchange.as_ref()?;
    let mut out = String::from("=== Captured exchange ===");
    if !ex.headers.is_empty() {
        out.push_str("\nheaders:");
        for (k, v) in &ex.headers {
            out.push_str(&format!("\n  {k}: {v}"));
        }
    }
    if !ex.trailers.is_empty() {
        out.push_str("\ntrailers:");
        for (k, v) in &ex.trailers {
            out.push_str(&format!("\n  {k}: {v}"));
        }
    }
    if !ex.response.is_empty() {
        out.push_str("\nresponse:");
        for msg in &ex.response {
            out.push('\n');
            out.push_str(&serde_json::to_string_pretty(msg).unwrap_or_else(|_| msg.to_string()));
        }
    }
    if ex.truncated {
        out.push_str("\n(response truncated)");
    }
    Some(out)
}

/// Build a detailed `<failure>` body listing each failed assertion with its
/// expected/actual diff. Returns `None` when no per-assertion detail is
/// available (the caller then falls back to the plain error message).
fn build_failure_body(result: &apif_state::TestResult) -> Option<String> {
    let failed: Vec<_> = result.assertions.iter().filter(|a| !a.passed).collect();
    if failed.is_empty() {
        return None;
    }
    let mut body = String::new();
    if let Some(msg) = &result.error_message {
        body.push_str(msg);
        body.push('\n');
    }
    for a in failed {
        body.push_str(&format!("\n  line {}: {}", a.line, a.expression));
        match (&a.expected, &a.actual) {
            (Some(e), Some(ac)) => {
                body.push_str(&format!("\n    expected: {e}\n    actual:   {ac}"));
            }
            _ => {
                if let Some(m) = &a.message {
                    body.push_str(&format!("\n    {m}"));
                }
            }
        }
    }
    Some(body)
}

/// `true` when a Fail result has no failed assertion on record — it failed
/// before ASSERTS ran (connection/timeout/parse/etc), which JUnit models as
/// `<error>` rather than `<failure>`.
fn is_execution_error(result: &apif_state::TestResult) -> bool {
    result.status == TestStatus::Fail && !result.assertions.iter().any(|a| !a.passed)
}

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
        let skipped = results.skipped();

        // Split Fail results into JUnit `failures` (an assertion was evaluated
        // and didn't hold) vs `errors` (failed before any assertion ran —
        // connection/timeout/parse/etc), instead of hardcoding errors="0".
        let (failures, errors): (usize, usize) = results
            .all()
            .iter()
            .filter(|r| r.status == TestStatus::Fail)
            .fold((0, 0), |(f, e), r| {
                if is_execution_error(r) {
                    (f, e + 1)
                } else {
                    (f + 1, e)
                }
            });

        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <testsuites name=\"grpctestify\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\">\n\
             <testsuite name=\"e2e\" time=\"{:.3}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\">\n",
            duration, total, failures, errors, skipped, duration, total, failures, errors, skipped
        );

        for result in results.all() {
            let display_name = result.meta.name.as_deref().unwrap_or(&result.name);
            let classname = std::path::Path::new(&result.name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".to_string());

            // Surface META owner/summary/links as properties (general data that
            // CI dashboards can index), alongside the existing tag properties.
            let mut extra_properties = Vec::new();
            if let Some(owner) = &result.meta.owner {
                extra_properties.push(("owner".to_string(), owner.clone()));
            }
            if let Some(summary) = &result.meta.summary {
                extra_properties.push(("summary".to_string(), summary.clone()));
            }
            for link in &result.meta.links {
                extra_properties.push(("link".to_string(), link.clone()));
            }

            let tc = TestCaseBuilder {
                name: display_name.to_string(),
                classname,
                duration_ms: result.duration_ms,
                status: result.status,
                error_message: result.error_message.clone(),
                failure_body: build_failure_body(result),
                system_out: build_system_out(result),
                is_execution_error: is_execution_error(result),
                tags: result.meta.tags.clone(),
                extra_properties,
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
    fn test_escape_xml_strips_invalid_control_chars() {
        // NUL, backspace, vertical tab, form feed, unit separator are invalid
        // in XML 1.0 and must be removed so the output stays parseable.
        let input = "a\u{0}b\u{8}c\u{b}d\u{c}e\u{1f}f";
        assert_eq!(escape_xml(input), "abcdef");
        // Tab, LF and CR are valid and must be preserved.
        assert_eq!(escape_xml("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn test_junit_failure_with_control_chars_is_well_formed() {
        // gRPC error text containing control chars must not leak into the XML.
        let tc = TestCaseBuilder {
            name: "ctrl".into(),
            classname: "suite".into(),
            duration_ms: 10,
            status: TestStatus::Fail,
            error_message: Some("boom\u{0}\u{1b}[31mred\u{7}".into()),
            failure_body: None,
            system_out: None,
            is_execution_error: false,
            tags: vec![],
            extra_properties: vec![],
        };
        let xml = tc.to_xml();
        assert!(
            !xml.chars()
                .any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r'))
        );
        assert!(xml.contains("boom"));
    }

    #[test]
    fn test_test_case_builder_to_xml_pass() {
        let tc = TestCaseBuilder {
            name: "test1".into(),
            classname: "suite".into(),
            duration_ms: 100,
            status: TestStatus::Pass,
            error_message: None,
            failure_body: None,
            system_out: None,
            is_execution_error: false,
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
            failure_body: None,
            system_out: None,
            is_execution_error: false,
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
            failure_body: None,
            system_out: None,
            is_execution_error: false,
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
            failure_body: None,
            system_out: None,
            is_execution_error: false,
            tags: vec!["api".into(), "smoke".into()],
            extra_properties: vec![("env".into(), "prod".into())],
        };
        let xml = tc.to_xml();
        assert!(xml.contains("properties"));
        assert!(xml.contains("api"));
        assert!(xml.contains("env"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_junit_reporter_lifecycle() {
        use crate::Reporter;
        use apif_state::TestResult;
        let path = std::env::temp_dir().join("test_junit_output.xml");
        let reporter = JunitReporter::new(path.clone());
        let mut results = TestResults::new();
        results.add(TestResult::pass("test.gctf", 100, None));
        assert!(reporter.on_suite_end(&results).is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_junit_failure_body_and_meta_properties() {
        use crate::Reporter;
        use apif_state::{AssertionRecord, TestMeta, TestResult};
        let path = std::env::temp_dir().join("test_junit_detail.xml");
        let reporter = JunitReporter::new(path.clone());
        let mut results = TestResults::new();
        let mut r = TestResult::fail("t.gctf", "1 assertion failed".into(), 10, None)
            .with_assertions(vec![AssertionRecord {
                line: 7,
                expression: ".status == active".into(),
                passed: false,
                elapsed_ms: 1,
                message: Some("mismatch".into()),
                endpoint: None,
                expected: Some("active".into()),
                actual: Some("pending".into()),
            }]);
        r.meta = TestMeta {
            owner: Some("team-a".into()),
            summary: Some("checks status".into()),
            links: vec!["http://x".into()],
            ..Default::default()
        };
        results.add(r);
        reporter.on_suite_end(&results).unwrap();

        let xml = std::fs::read_to_string(&path).unwrap();
        assert!(xml.contains("expected: active"), "failure body diff: {xml}");
        assert!(
            xml.contains("actual:   pending"),
            "failure body diff: {xml}"
        );
        assert!(xml.contains("line 7:"), "failure body line: {xml}");
        assert!(
            xml.contains("name=\"owner\" value=\"team-a\""),
            "owner prop: {xml}"
        );
        assert!(xml.contains("name=\"summary\""), "summary prop: {xml}");
        assert!(
            xml.contains("name=\"link\" value=\"http://x\""),
            "link prop: {xml}"
        );
        let _ = std::fs::remove_file(&path);
    }

    // Regression: a connection/timeout failure (no assertion evaluated) must be
    // reported as JUnit <error>, not <failure> — and an assertion failure must
    // stay <failure>. The suite-level errors="N" must no longer be hardcoded 0.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_junit_splits_errors_from_assertion_failures() {
        use crate::Reporter;
        use apif_state::{AssertionRecord, TestResult};
        let path = std::env::temp_dir().join("test_junit_errors_vs_failures.xml");
        let reporter = JunitReporter::new(path.clone());
        let mut results = TestResults::new();

        // Connection error: Fail status, no assertions recorded at all.
        results.add(TestResult::fail(
            "conn.gctf",
            "connection refused".into(),
            5,
            None,
        ));
        // Assertion failure: Fail status, a failed assertion on record.
        results.add(
            TestResult::fail("assert.gctf", "1 assertion failed".into(), 5, None).with_assertions(
                vec![AssertionRecord {
                    line: 3,
                    expression: ".ok == true".into(),
                    passed: false,
                    elapsed_ms: 1,
                    message: None,
                    endpoint: None,
                    expected: Some("true".into()),
                    actual: Some("false".into()),
                }],
            ),
        );

        reporter.on_suite_end(&results).unwrap();
        let xml = std::fs::read_to_string(&path).unwrap();

        assert!(
            xml.contains("tests=\"2\" failures=\"1\" errors=\"1\""),
            "suite header must split failures/errors: {xml}"
        );
        assert!(
            xml.contains("<error message=\"connection refused\""),
            "connection failure must be <error>: {xml}"
        );
        assert!(
            xml.contains("<failure message=\"1 assertion failed\""),
            "assertion failure must be <failure>: {xml}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_junit_system_out_from_exchange() {
        use crate::Reporter;
        use apif_state::{CapturedExchange, TestResult};
        use std::collections::BTreeMap;
        let path = std::env::temp_dir().join("test_junit_sysout.xml");
        let reporter = JunitReporter::new(path.clone());
        let mut results = TestResults::new();
        let ex = CapturedExchange {
            headers: BTreeMap::from([("content-type".to_string(), "application/grpc".to_string())]),
            trailers: BTreeMap::new(),
            response: vec![serde_json::json!({"id": 1})],
            truncated: false,
        };
        results.add(TestResult::pass("t.gctf", 5, None).with_exchange(Some(ex)));
        reporter.on_suite_end(&results).unwrap();

        let xml = std::fs::read_to_string(&path).unwrap();
        assert!(xml.contains("<system-out>"), "system-out element: {xml}");
        assert!(xml.contains("Captured exchange"), "exchange body: {xml}");
        assert!(xml.contains("content-type"), "header in body: {xml}");
        let _ = std::fs::remove_file(&path);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_junit_reporter_with_failure() {
        use crate::Reporter;
        use apif_state::TestResult;
        let path = std::env::temp_dir().join("test_junit_fail.xml");
        let reporter = JunitReporter::new(path.clone());
        let mut results = TestResults::new();
        results.add(TestResult::fail("test.gctf", "error msg".into(), 100, None));
        assert!(reporter.on_suite_end(&results).is_ok());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("failure"));
        assert!(content.contains("error msg"));
        let _ = std::fs::remove_file(&path);
    }
}
