#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe
use super::Reporter;
use anyhow::{Context, Result};
use apif_state::{TestResults, TestStatus};
use serde::Serialize;
use std::cmp::Reverse;
use std::fs;
use std::path::PathBuf;

const TEMPLATE: &str = include_str!("../templates/report.html");

pub struct HtmlReporter {
    output_path: PathBuf,
}

impl HtmlReporter {
    pub fn new(output_path: PathBuf) -> Self {
        Self { output_path }
    }
}

#[derive(Serialize)]
struct TestRow<'a> {
    name: &'a str,
    duration_ms: u64,
    /// Bar width as a percentage of the slowest test, for the CSS bar chart.
    pct_of_max: u32,
}

/// Percentage (0-100, rounded) of `value` relative to `max`. `0` when `max` is
/// `0` so an all-zero dataset renders empty bars instead of dividing by zero.
fn pct_of(value: u64, max: u64) -> u32 {
    if max == 0 {
        0
    } else {
        ((value as f64 / max as f64) * 100.0).round() as u32
    }
}

/// `"2026-07-22T14:03:11.123456+00:00"` (chrono's UTC RFC3339) rendered as
/// `"2026-07-22 14:03:11 UTC"` — sub-second precision and the numeric offset
/// don't help a reader eyeballing when this report was generated.
fn format_generated_at(rfc3339: &str) -> String {
    let date_time = rfc3339
        .split('.')
        .next()
        .unwrap_or(rfc3339)
        .replace('T', " ");
    format!("{date_time} UTC")
}

#[derive(Serialize)]
struct AssertionRow<'a> {
    test_name: &'a str,
    line: usize,
    expression: &'a str,
    elapsed_ms: u64,
    pct_of_max: u32,
}

/// One assertion line inside a test detail card — every assertion the test
/// evaluated (not only the failed ones), so the reader sees the full picture
/// of what was checked and can spot which line actually broke.
#[derive(Serialize)]
struct AssertionDetail<'a> {
    line: usize,
    expression: &'a str,
    passed: bool,
    expected: Option<&'a str>,
    actual: Option<&'a str>,
    message: Option<&'a str>,
    elapsed_ms: u64,
    endpoint: Option<&'a str>,
}

/// The captured request/response exchange, rendered inside a native
/// `<details>` disclosure so it's available without cluttering the default
/// view — no JS needed for expand/collapse.
#[derive(Serialize)]
struct ExchangeDetail {
    headers: Vec<(String, String)>,
    trailers: Vec<(String, String)>,
    response_json: String,
    truncated: bool,
}

/// One test as a self-contained detail card: identity, META, every
/// assertion, and (if captured) the exchange — everything needed to diagnose
/// a failure without cross-referencing another table.
#[derive(Serialize)]
struct TestCard<'a> {
    status_icon: &'a str,
    status_class: &'a str,
    name: &'a str,
    duration_ms: u64,
    /// gRPC call time alone, when the runner recorded it separately from
    /// `duration_ms` (which also includes parse/assert overhead).
    call_duration_ms: Option<u64>,
    tags: &'a [String],
    owner: Option<&'a str>,
    error: Option<&'a str>,
    assertions: Vec<AssertionDetail<'a>>,
    exchange: Option<ExchangeDetail>,
    /// `true` when at least one request needed a retry to succeed.
    retried: bool,
    /// Per-document wall-clock time for a multi-document `.gctf` chain, in
    /// source order. Empty for a single-document test.
    document_durations_ms: &'a [u64],
    /// This case's `--data`/`DATASET` row values, when the test was expanded
    /// from a data source.
    row_params: &'a [(String, String)],
    /// Expanded by default when the test failed — diagnosing a failure is why
    /// this report gets opened, so that card shouldn't need an extra click.
    open: bool,
    /// `false` when there is nothing to show below the summary line (a passed
    /// test with no assertions/owner/exchange/etc.) — such a card renders as
    /// a plain row instead of a `<details>` with a misleading expand arrow
    /// that opens onto an empty body.
    has_detail: bool,
}

#[derive(Serialize)]
struct ReportContext<'a> {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    duration_ms: u64,
    /// Human-friendly duration for the hero's big number: `"842ms"` below one
    /// second, `"4.30s"` at or above it — a raw millisecond count reads as
    /// noise once a suite takes multiple seconds.
    duration_display: String,
    /// Share of the hero ratio bar each status occupies — a slim CSS bar
    /// instead of a full donut chart, so the verdict banner stays one glance
    /// instead of competing with a separate chart section for attention.
    pass_pct: u32,
    fail_pct: u32,
    skip_pct: u32,
    /// Every test, once — failed first (expanded), then the rest (collapsed).
    /// A single list instead of separate "Failed"/"All" sections, so no test
    /// is ever rendered twice.
    tests: Vec<TestCard<'a>>,
    slowest_tests: Vec<TestRow<'a>>,
    slowest_assertions: Vec<AssertionRow<'a>>,
    /// When this report was rendered, e.g. `"2026-07-22 14:03:11 UTC"` — the
    /// suite's own duration/timing is about the *run*, this is about the
    /// *report file* itself (stale-report detection at a glance).
    generated_at: String,
}

fn status_icon_class(status: TestStatus) -> (&'static str, &'static str) {
    match status {
        TestStatus::Pass => (crate::style::PASS_ICON, "status-pass"),
        TestStatus::Fail => (crate::style::FAIL_ICON, "status-fail"),
        TestStatus::Skip => (crate::style::SKIP_ICON, "status-skip"),
    }
}

fn build_test_card(r: &apif_state::TestResult) -> TestCard<'_> {
    let (status_icon, status_class) = status_icon_class(r.status);
    let assertions: Vec<AssertionDetail> = r
        .assertions
        .iter()
        .map(|a| AssertionDetail {
            line: a.line,
            expression: &a.expression,
            passed: a.passed,
            expected: a.expected.as_deref(),
            actual: a.actual.as_deref(),
            message: a.message.as_deref(),
            elapsed_ms: a.elapsed_ms,
            endpoint: a.endpoint.as_deref(),
        })
        .collect();
    let exchange = r.exchange.as_ref().map(|ex| {
        let mut headers: Vec<_> = ex.headers.clone().into_iter().collect();
        headers.sort();
        let mut trailers: Vec<_> = ex.trailers.clone().into_iter().collect();
        trailers.sort();
        ExchangeDetail {
            headers,
            trailers,
            response_json: serde_json::to_string_pretty(&ex.response)
                .unwrap_or_else(|_| "[]".to_string()),
            truncated: ex.truncated,
        }
    });
    let owner = r.meta.owner.as_deref();
    let error = r.error_message.as_deref();
    let has_detail = owner.is_some()
        || error.is_some()
        || !assertions.is_empty()
        || exchange.is_some()
        || r.retried
        || !r.document_durations_ms.is_empty()
        || !r.row_params.is_empty();
    TestCard {
        status_icon,
        status_class,
        name: r.meta.name.as_deref().unwrap_or(&r.name),
        duration_ms: r.duration_ms,
        call_duration_ms: r.call_duration_ms,
        tags: &r.meta.tags,
        owner,
        error,
        assertions,
        exchange,
        retried: r.retried,
        document_durations_ms: &r.document_durations_ms,
        row_params: &r.row_params,
        open: r.status == TestStatus::Fail,
        has_detail,
    }
}

impl Reporter for HtmlReporter {
    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        let all = results.all();
        let total = results.total();
        let passed = results.passed();
        let failed = results.failed();
        let skipped = results.skipped();
        let metrics = results.metrics();

        let total_u64 = total as u64;
        let pass_pct = pct_of(passed as u64, total_u64);
        let fail_pct = pct_of(failed as u64, total_u64);
        let skip_pct = pct_of(skipped as u64, total_u64);

        let mut by_duration = all.iter().collect::<Vec<_>>();
        by_duration.sort_by_key(|r| Reverse(r.duration_ms));
        let slowest_duration_max = by_duration.first().map(|r| r.duration_ms).unwrap_or(0);
        let slowest_tests: Vec<TestRow> = by_duration
            .iter()
            .take(10)
            .map(|r| TestRow {
                name: r.meta.name.as_deref().unwrap_or(&r.name),
                duration_ms: r.duration_ms,
                pct_of_max: pct_of(r.duration_ms, slowest_duration_max),
            })
            .collect();

        let mut slowest_assertions: Vec<AssertionRow> = all
            .iter()
            .flat_map(|r| {
                let name = r.meta.name.as_deref().unwrap_or(&r.name);
                r.assertions.iter().map(move |a| AssertionRow {
                    test_name: name,
                    line: a.line,
                    expression: &a.expression,
                    elapsed_ms: a.elapsed_ms,
                    pct_of_max: 0,
                })
            })
            .collect();
        slowest_assertions.sort_by_key(|a| Reverse(a.elapsed_ms));
        slowest_assertions.truncate(10);
        let assertion_elapsed_max = slowest_assertions
            .first()
            .map(|a| a.elapsed_ms)
            .unwrap_or(0);
        for a in &mut slowest_assertions {
            a.pct_of_max = pct_of(a.elapsed_ms, assertion_elapsed_max);
        }

        // One self-contained card per test (error + every assertion +
        // tags/owner + captured exchange) instead of scattering a failure
        // across separate tables. Failed tests sort first (and start
        // expanded) since diagnosing them is the primary reason to open this
        // report; a stable sort keeps everything else in its original order.
        let mut tests: Vec<TestCard> = all.iter().map(build_test_card).collect();
        tests.sort_by_key(|t| !t.open);

        let ctx = ReportContext {
            total,
            passed,
            failed,
            skipped,
            duration_ms: metrics.total_duration_ms,
            duration_display: crate::style::format_duration_ms(metrics.total_duration_ms),
            pass_pct,
            fail_pct,
            skip_pct,
            tests,
            slowest_tests,
            slowest_assertions,
            generated_at: format_generated_at(&apif_cfg_runtime::now_rfc3339()),
        };

        let mut env = minijinja::Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        env.add_template("report.html", TEMPLATE)
            .context("invalid built-in HTML report template")?;
        let rendered = env
            .get_template("report.html")
            .unwrap()
            .render(&ctx)
            .context("failed to render HTML report")?;

        fs::write(&self.output_path, rendered).with_context(|| {
            format!(
                "Failed to write HTML report file: {}",
                self.output_path.display()
            )
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apif_state::{AssertionRecord, TestResult};

    #[test]
    fn html_reporter_new() {
        let reporter = HtmlReporter::new(PathBuf::from("report.html"));
        assert_eq!(reporter.output_path.to_str(), Some("report.html"));
    }

    #[test]
    fn pct_of_all_zero_does_not_divide_by_zero() {
        assert_eq!(pct_of(0, 0), 0);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_renders_fail_hero_with_jump_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(TestResult::pass("a.gctf", 10, None));
        results.add(TestResult::fail("b.gctf", "boom".into(), 10, None));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hero-fail"), "fail hero shown: {content}");
        assert!(
            content.contains("1 of 2 tests failed"),
            "verdict title present: {content}"
        );
        assert!(
            content.contains(r##"href="#tests""##),
            "jump-to-failures link present: {content}"
        );
        assert!(!content.contains("<svg"), "no donut chart: {content}");
        assert!(!content.contains("<canvas"), "no canvas element: {content}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_renders_pass_hero_without_jump_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(TestResult::pass("a.gctf", 10, None));
        results.add(TestResult::pass("b.gctf", 10, None));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hero-pass"), "pass hero shown: {content}");
        assert!(
            content.contains("All 2 tests passed"),
            "verdict title present: {content}"
        );
        assert!(
            !content.contains(r##"href="#tests""##),
            "no jump link on a clean pass: {content}"
        );
        // A fully green run collapses the tests list by default so the page
        // stays short — only the count is visible until expanded.
        assert!(
            content.contains(r#"<summary class="section-toggle">Tests (2)</summary>"#),
            "tests collapsed under a summary: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_renders_captured_exchange_collapsible() {
        use apif_state::CapturedExchange;
        use std::collections::BTreeMap;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        let ex = CapturedExchange {
            headers: BTreeMap::from([("content-type".to_string(), "application/grpc".to_string())]),
            trailers: BTreeMap::new(),
            response: vec![serde_json::json!({"id": 42})],
            truncated: false,
        };
        results.add(TestResult::fail("t.gctf", "boom".into(), 5, None).with_exchange(Some(ex)));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // A native <details> disclosure — no JS needed to expand/collapse.
        assert!(
            content.contains(r#"<details class="exchange">"#),
            "collapsible exchange: {content}"
        );
        // minijinja's HTML autoescaper escapes `/` and `"` even inside these
        // otherwise-safe contexts, matching the escaping used everywhere else.
        assert!(content.contains("content-type: application&#x2f;grpc"));
        assert!(content.contains("&quot;id&quot;: 42"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_all_tests_shows_full_list_with_tags_and_owner() {
        use apif_state::TestMeta;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        let mut passed = TestResult::pass("users.Get.gctf", 8, None);
        passed.meta = TestMeta {
            tags: vec!["smoke".to_string()],
            owner: Some("team-users".to_string()),
            ..Default::default()
        };
        results.add(passed);
        results.add(TestResult::pass("users.List.gctf", 12, None));

        reporter.on_suite_end(&results).unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert!(content.contains("Tests (2)"));
        // Both tests appear even though neither is "slowest" or failed.
        assert!(content.contains("users.Get.gctf"));
        assert!(content.contains("users.List.gctf"));
        assert!(
            content.contains("class=\"tag\">smoke<"),
            "tag shown: {content}"
        );
        assert!(content.contains("team-users"), "owner shown: {content}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_escapes_title_attribute_tooltips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        // A name containing a double quote and an angle bracket must not break
        // out of the `title="..."` attribute or the enclosing `<div>` tag.
        results.add(TestResult::pass(
            r#"evil" onmouseover="alert(1)<b>.gctf"#,
            5,
            None,
        ));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains(r#"onmouseover="alert(1)"#),
            "raw handler must not appear unescaped: {content}"
        );
        assert!(content.contains("&quot;"), "quote escaped: {content}");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_writes_valid_report_with_bars_and_escaping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(
            TestResult::pass("safe.gctf", 42, Some(10)).with_assertions(vec![AssertionRecord {
                line: 3,
                expression: ".id == 1".to_string(),
                passed: true,
                elapsed_ms: 5,
                message: None,
                endpoint: None,
                expected: None,
                actual: None,
            }]),
        );
        results.add(TestResult::fail(
            "<script>evil.gctf</script>",
            "boom & <bang>".to_string(),
            99,
            None,
        ));

        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // No JS charting library or CDN fetch — the report is a single
        // self-contained file that renders fully offline, and there is no
        // `<script>` block left for an injected name/error to break out of.
        assert!(!content.contains("<script"), "no script tag: {content}");
        assert!(!content.contains("cdn.jsdelivr.net"));
        assert!(content.contains("bar-fill"), "CSS bar chart present");
        assert!(content.contains("safe.gctf"));
        // The malicious name/error must be escaped, not injected verbatim.
        assert!(!content.contains("<script>evil.gctf</script>"));
        assert!(content.contains("&lt;script&gt;evil.gctf&lt;&#x2f;script&gt;"));
        assert!(content.contains("boom &amp; &lt;bang&gt;"));
        assert!(content.contains(".id == 1"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_renders_failed_assertion_diffs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(
            TestResult::fail("t.gctf", "1 assertion failed".into(), 10, None).with_assertions(
                vec![
                    AssertionRecord {
                        line: 5,
                        expression: ".status == active".to_string(),
                        passed: false,
                        elapsed_ms: 1,
                        message: None,
                        endpoint: None,
                        expected: Some("active".to_string()),
                        actual: Some("pending".to_string()),
                    },
                    AssertionRecord {
                        line: 6,
                        expression: ".id != null".to_string(),
                        passed: true,
                        elapsed_ms: 1,
                        message: None,
                        endpoint: None,
                        expected: None,
                        actual: None,
                    },
                ],
            ),
        );
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("<h2>Tests"), "section present: {content}");
        // The failed test's card must be open (visible) by default.
        assert!(
            content.contains(r#"<details class="test-card status-fail" open>"#),
            "failed card starts expanded: {content}"
        );
        assert!(content.contains(".status == active"));
        assert!(content.contains("active"));
        assert!(content.contains("pending"));
        // The failed test's card shows every assertion (not only the failed
        // one) so the reader sees the full picture — the passed assertion
        // legitimately appears too, marked distinctly (a-pass vs a-fail).
        assert!(
            content.contains(".id != null"),
            "passed assertion shown too: {content}"
        );
        assert!(
            content.contains("tr class=\"a-fail\""),
            "failed row marked: {content}"
        );
        assert!(
            content.contains("tr class=\"a-pass\""),
            "passed row marked: {content}"
        );
        // Regression: a passed assertion has no expected/actual (both None on
        // AssertionRecord). Rust's None serializes to JSON null, and
        // minijinja's `default` filter only substitutes for *undefined*, not
        // null — so a naive `{{ x|default('') }}` would literally print the
        // word "none" into the Expected/Actual cells instead of leaving them
        // blank. Assert the empty cell, not the leaked literal.
        assert!(
            !content.contains(">none<"),
            "must not leak the literal word 'none' into an empty cell: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_has_light_dark_toggle_and_no_test_duplication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(TestResult::fail("dup-check.gctf", "boom".into(), 5, None));
        results.add(TestResult::pass("other.gctf", 5, None));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();

        // Zero-JS light/dark toggle: a checkbox whose `:checked` state flips
        // the theme via a CSS sibling selector — no separate "Failed"/"All"
        // sections to keep in sync with a light/dark class list either.
        assert!(
            content.contains(r#"<input type="checkbox" id="theme-toggle""#),
            "theme toggle checkbox present: {content}"
        );
        assert!(
            content.contains("#theme-toggle:checked ~ .page"),
            "checked state overrides theme variables: {content}"
        );

        // Regression: a failed test's full detail card must render exactly
        // once — there used to be a separate "Failed Tests" + "All Tests"
        // section, so the same card (error box, assertions, etc.) rendered
        // twice. (Its name legitimately also appears in "Slowest Tests" — a
        // different, top-N-by-duration view — so we count the error box, not
        // the bare name, to isolate the full-card duplication specifically.)
        let card_occurrences = content
            .matches(r#"<div class="error-box">boom</div>"#)
            .count();
        assert_eq!(
            card_occurrences, 1,
            "failed test's detail card must render once, not duplicated: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_bare_pass_renders_plain_row_not_empty_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        // No assertions, no owner, no error, no exchange — nothing to expand.
        results.add(TestResult::pass("bare.gctf", 5, None));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains(r#"<div class="test-card test-card-plain status-pass">"#),
            "renders as a plain row, not a <details> with an empty body: {content}"
        );
        assert!(
            !content.contains("<details class=\"test-card\""),
            "no expandable card when there's nothing to expand: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_shows_generated_at_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(TestResult::pass("a.gctf", 10, None));
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("generated ") && content.contains(" UTC"),
            "report must show when it was generated: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_shows_call_duration_retried_and_row_params() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(
            TestResult::pass("slow.gctf", 120, Some(80))
                .with_retried(true)
                .with_document_durations(vec![30, 90])
                .with_row_params(vec![("id".to_string(), "42".to_string())]),
        );
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("80ms of 120ms total"),
            "call duration shown distinctly from total: {content}"
        );
        assert!(
            content.contains(r#"<span class="meta-key">Retried</span>yes"#),
            "retried badge shown: {content}"
        );
        assert!(
            content.contains("id=42"),
            "data-driven row params shown: {content}"
        );
        assert!(
            content.contains("#1 30ms, #2 90ms"),
            "per-document chain timing shown: {content}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_shows_per_assertion_timing_and_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.html");
        let reporter = HtmlReporter::new(path.clone());

        let mut results = TestResults::new();
        results.add(
            TestResult::pass("t.gctf", 10, None).with_assertions(vec![AssertionRecord {
                line: 3,
                expression: ".id == 1".to_string(),
                passed: true,
                elapsed_ms: 7,
                message: None,
                endpoint: Some("pkg.Svc/Method".to_string()),
                expected: None,
                actual: None,
            }]),
        );
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("<th>Time</th>"), "time column: {content}");
        assert!(
            content.contains("<th>Endpoint</th>"),
            "endpoint column: {content}"
        );
        assert!(content.contains("pkg.Svc&#x2f;Method"));
        assert!(content.contains(">7ms<"));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn html_reporter_handles_empty_results() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.html");
        let reporter = HtmlReporter::new(path.clone());

        let results = TestResults::new();
        reporter.on_suite_end(&results).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("<html"));
        assert!(
            content.contains("hero-empty"),
            "neutral hero for an empty suite: {content}"
        );
        assert!(content.contains("No tests ran"));
    }
}
