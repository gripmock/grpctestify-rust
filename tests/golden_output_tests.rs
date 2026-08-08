#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Golden/snapshot coverage for every `run` output form. Non-deterministic
//! content (timestamps, durations, temp paths, uuids, hostnames) is scrubbed
//! to stable placeholders and compared against `tests/golden/`.
//! `--parallel 1` avoids scheduling-order/timing-tie flakiness.
//! Regenerate: `UPDATE_GOLDEN=1 cargo test --test golden_output_tests`

#[path = "support/mod.rs"]
mod support;
use support::spawn_health_server;

use std::path::Path;

/// One passing and one failing `.gctf`, same shape as `plugin_dir_tests.rs`'s
/// fixtures — `a_`/`b_` prefixes make file-path sort order (the CLI's
/// default `--sort path`) deterministic regardless of which finishes first.
fn write_pair(dir: &Path, address: &str) {
    std::fs::write(
        dir.join("a_pass.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .expect("write a_pass.gctf");
    std::fs::write(
        dir.join("b_fail.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"NOT_SERVING\"\n"
        ),
    )
    .expect("write b_fail.gctf");
}

fn write_pass_only(dir: &Path, address: &str) {
    std::fs::write(
        dir.join("a_pass.gctf"),
        format!(
            "--- ADDRESS ---\n{address}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{{}}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n"
        ),
    )
    .expect("write a_pass.gctf");
}

/// Collapse run-to-run-variable substrings to fixed placeholders.
fn scrub(text: &str, tmp_root: &Path) -> String {
    let mut s = text.to_string();

    let tmp_root_str = tmp_root.to_string_lossy().into_owned();
    s = s.replace(&tmp_root_str, "<TMPDIR>");
    // The html reporter percent-encodes path separators as HTML entities.
    s = s.replace(&tmp_root_str.replace('/', "&#x2f;"), "<TMPDIR>");

    // `tmp_root` is a raw OS path (single `\` on Windows), but JSON report
    // bodies (allure, json) escape every `\` as `\\` — the exact-match
    // replace above never matches there. Try the JSON-escaped form too.
    let json_escaped = tmp_root_str.replace('\\', "\\\\");
    if json_escaped != tmp_root_str {
        s = s.replace(&json_escaped, "<TMPDIR>");
    }
    // Whichever form matched, the separator joining `<TMPDIR>` to the
    // filename wasn't part of `tmp_root` itself, so it survives untouched:
    // single `\` (Windows, non-JSON reports — the html reporter's own
    // percent-encoding only ever targets a real `/`, never `\`) or
    // JSON-escaped-doubled `\\` (Windows, json/allure). Normalize it to
    // whatever separator style the rest of this same report already uses —
    // `&#x2f;` if this is html output (detectable from its own unrelated,
    // platform-independent escaping, e.g. `Health&#x2f;Check`), else plain
    // `/` — so it matches the Unix-generated golden either way.
    let target_sep = if s.contains("&#x2f;") { "&#x2f;" } else { "/" };
    s = s.replace("<TMPDIR>\\\\", &format!("<TMPDIR>{target_sep}"));
    s = s.replace("<TMPDIR>\\", &format!("<TMPDIR>{target_sep}"));

    let http_date =
        regex::Regex::new(r"[A-Za-z]{3}, \d{2} [A-Za-z]{3} \d{4} \d{2}:\d{2}:\d{2} GMT").unwrap();
    s = http_date.replace_all(&s, "<HTTP_DATE>").into_owned();

    let iso_ts =
        regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(\+\d{2}:\d{2}|Z)").unwrap();
    s = iso_ts.replace_all(&s, "<ISO_TS>").into_owned();

    let ts_utc = regex::Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC").unwrap();
    s = ts_utc.replace_all(&s, "<TS>").into_owned();

    // UUID-shaped tokens — real UUIDs (allure result/attachment ids) and
    // allure's own uuid-formatted history/test-case hashes alike.
    let uuid = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .unwrap();
    s = uuid.replace_all(&s, "<UUID>").into_owned();

    let epoch_ms = regex::Regex::new(r"\b\d{13}\b").unwrap();
    s = epoch_ms.replace_all(&s, "<EPOCH_MS>").into_owned();
    let epoch_s = regex::Regex::new(r"\b\d{10}\b").unwrap();
    s = epoch_s.replace_all(&s, "<EPOCH_S>").into_owned();

    let ms = regex::Regex::new(r"\b\d+(\.\d+)?ms\b").unwrap();
    s = ms.replace_all(&s, "<MS>ms").into_owned();

    let junit_time = regex::Regex::new(r#"time="[\d.]+""#).unwrap();
    s = junit_time.replace_all(&s, r#"time="<N>""#).into_owned();

    // Scalar timing fields in JSON/YAML report bodies.
    let scalar_keys = [
        "duration_ms",
        "call_duration_ms",
        "elapsed_ms",
        "grpcDuration",
        "duration",
        "total_duration_ms",
        "start_time",
        "end_time",
        "total_rpc_ms",
        "sum_test_ms",
    ];
    let scalar_pattern = format!(r#"("(?:{})"\s*:\s*)-?\d+"#, scalar_keys.join("|"));
    let scalar_re = regex::Regex::new(&scalar_pattern).unwrap();
    s = scalar_re.replace_all(&s, "${1}0").into_owned();
    let yaml_scalar_pattern = format!(
        r"(?m)^(\s*(?:{}):\s*)-?\d+$",
        scalar_keys
            .join("|")
            .replace("grpcDuration", "grpc_duration")
    );
    let yaml_scalar_re = regex::Regex::new(&yaml_scalar_pattern).unwrap();
    s = yaml_scalar_re.replace_all(&s, "${1}0").into_owned();

    // `document_durations_ms`: a per-document array, JSON and YAML shapes.
    let doc_durations_json =
        regex::Regex::new(r#""document_durations_ms"\s*:\s*\[[^\]]*\]"#).unwrap();
    s = doc_durations_json
        .replace_all(&s, r#""document_durations_ms": [0]"#)
        .into_owned();
    let doc_durations_yaml =
        regex::Regex::new(r"(?m)^([ \t]*)document_durations_ms:\n(?:[ \t]*- -?\d+\n)+").unwrap();
    s = doc_durations_yaml
        .replace_all(&s, "${1}document_durations_ms:\n${1}- 0\n")
        .into_owned();

    let host = regex::Regex::new(r#""host"\s*,\s*"value"\s*:\s*"[^"]*""#).unwrap();
    s = host
        .replace_all(&s, r#""host","value":"<HOST>""#)
        .into_owned();

    let thread = regex::Regex::new(r"ThreadId\(\d+\)").unwrap();
    s = thread.replace_all(&s, "ThreadId(<N>)").into_owned();

    let pct = regex::Regex::new(r"width:\d+(\.\d+)?%").unwrap();
    s = pct.replace_all(&s, "width:<PCT>%").into_owned();

    let version = regex::Regex::new(r#""(?:version|grpctestify_version)"\s*:\s*"[^"]*""#).unwrap();
    s = version
        .replace_all(&s, r#""version":"<VERSION>""#)
        .into_owned();
    // Allure carries the version as a name/value pair, which the plain
    // `"version":` form above does not reach.
    let version_pair = regex::Regex::new(
        r#""(grpctestify_version|grpctestify\.version)"\s*,\s*"value"\s*:\s*"[^"]*""#,
    )
    .unwrap();
    s = version_pair
        .replace_all(&s, r#""${1}","value":"<VERSION>""#)
        .into_owned();

    let version_yaml = regex::Regex::new(r"(?m)^(\s*version:\s*)\S+$").unwrap();
    s = version_yaml.replace_all(&s, "${1}<VERSION>").into_owned();

    let version_props = regex::Regex::new(r"grpctestify\.version=\S+").unwrap();
    s = version_props
        .replace_all(&s, "grpctestify.version=<VERSION>")
        .into_owned();

    // "Call Xms of Yms total" vanishes entirely (not "0ms") when the call
    // rounds to 0ms — drop the line rather than scrub around it. `.*?`, not
    // `[^<]*`: `<MS>` itself contains `<`.
    let call_meta = regex::Regex::new(
        r#"<div class="meta-line"><span class="meta-key">Call</span>.*?</div>\n"#,
    )
    .unwrap();
    s = call_meta.replace_all(&s, "").into_owned();

    // Stripping that optional line leaves one fewer blank line than the
    // template's own `{% if %}`-false path does natively — collapse any run
    // of blank lines to one rather than chase every such asymmetry
    // individually; blank-line *count* isn't meaningful HTML content.
    let blank_runs = regex::Regex::new(r"\n{3,}").unwrap();
    s = blank_runs.replace_all(&s, "\n\n").into_owned();

    s
}

/// Sort duration-ordered bar rows (html's "Slowest Tests"/"Slowest
/// Assertions") so jitter-driven reordering doesn't flip a comparison.
fn sort_bar_rows(text: &str) -> String {
    let bar_row = regex::Regex::new(r#"<div class="bar-row"[^\n]*</div>\n"#).unwrap();
    let mut rows: Vec<&str> = bar_row.find_iter(text).map(|m| m.as_str()).collect();
    if rows.is_empty() {
        return text.to_string();
    }
    rows.sort_unstable();
    let mut out = String::new();
    let mut last_end = 0;
    for (row_idx, m) in bar_row.find_iter(text).enumerate() {
        out.push_str(&text[last_end..m.start()]);
        out.push_str(rows[row_idx]);
        last_end = m.end();
    }
    out.push_str(&text[last_end..]);
    out
}

fn assert_golden(name: &str, actual: &str) {
    support::assert_golden(&format!("output_forms/{name}.golden"), actual);
}

async fn run_console_form(name: &str, progress: &str, stream: bool) {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    write_pair(dir.path(), &address);

    let mut args = vec!["run", "--parallel", "1", "--progress", progress];
    if stream {
        args.push("--stream");
    }
    args.push(dir.path().to_str().unwrap());

    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args(&args)
        .output()
        .expect("failed to run CLI");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a_pass+b_fail mix must exit 1 for '{name}'\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "=== STDOUT ===\n{}\n=== STDERR ===\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden(name, &scrub(&combined, dir.path()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_dots() {
    run_console_form("run_dots", "dots", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_verbose() {
    run_console_form("run_verbose", "verbose", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_silent() {
    run_console_form("run_silent", "none", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_stream_ndjson() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    write_pair(dir.path(), &address);

    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .args([
            "run",
            "--parallel",
            "1",
            "--progress",
            "none",
            "--stream",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run CLI");

    assert_eq!(output.status.code(), Some(1));

    // Each line is a standalone JSON event whose key order is stable
    // (serde field order) but whose *field values* (timestamps, durations,
    // temp paths) aren't — scrub the whole stream as one blob.
    assert_golden(
        "run_stream_ndjson",
        &scrub(&String::from_utf8_lossy(&output.stdout), dir.path()),
    );
}

async fn run_report_form(name: &str, format: &str) {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    write_pair(dir.path(), &address);
    let out_path = dir.path().join(format!("report.{format}"));

    let output = support::cli_command()
        .env("NO_COLOR", "1")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("CI")
        .args([
            "run",
            "--parallel",
            "1",
            "--progress",
            "none",
            "--log-format",
            format,
            "--log-output",
            out_path.to_str().unwrap(),
            dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run CLI");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a_pass+b_fail mix must exit 1 for '{name}'\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let content = if out_path.is_dir() {
        // Allure: one directory of files with random uuid-based names.
        // Sort by *scrubbed content*, not filename — filenames embed a
        // random uuid, so sorting by name would itself be nondeterministic
        // across runs, unlike sorting by what the file actually says.
        let mut entries: Vec<(String, String)> = std::fs::read_dir(&out_path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| {
                let raw = std::fs::read_to_string(&p).unwrap_or_default();
                let label = p.file_name().unwrap().to_string_lossy().into_owned();
                (scrub(&label, dir.path()), scrub(&raw, dir.path()))
            })
            .collect();
        entries.sort_unstable();
        entries
            .into_iter()
            .map(|(label, body)| format!("=== {label} ===\n{body}\n"))
            .collect::<String>()
    } else {
        std::fs::read_to_string(&out_path).unwrap_or_default()
    };

    let scrubbed = scrub(&content, dir.path());
    let scrubbed = if format == "html" {
        sort_bar_rows(&scrubbed)
    } else {
        scrubbed
    };
    assert_golden(name, &scrubbed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_json() {
    run_report_form("run_json", "json").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_yaml() {
    run_report_form("run_yaml", "yaml").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_junit() {
    run_report_form("run_junit", "junit").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_html() {
    run_report_form("run_html", "html").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn golden_allure() {
    run_report_form("run_allure", "allure").await;
}

/// §2.3: every form must exit 0 on an all-passing suite, independent of the
/// golden content checks above (which deliberately use a mixed pass+fail
/// suite to exercise both rendering paths).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_code_is_zero_on_an_all_passing_suite_for_every_form() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    write_pass_only(dir.path(), &address);

    let console_forms: &[(&str, bool)] = &[("dots", false), ("verbose", false), ("none", false)];
    for (progress, _) in console_forms {
        let output = support::cli_command()
            .env("NO_COLOR", "1")
            .args([
                "run",
                "--parallel",
                "1",
                "--progress",
                progress,
                dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "progress={progress} must exit 0 on an all-passing suite\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stream_output = support::cli_command()
        .env("NO_COLOR", "1")
        .args([
            "run",
            "--parallel",
            "1",
            "--progress",
            "none",
            "--stream",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        stream_output.status.code(),
        Some(0),
        "--stream must exit 0 on an all-passing suite"
    );

    for format in ["json", "yaml", "junit", "html", "allure"] {
        let out_path = dir.path().join(format!("report-pass.{format}"));
        let output = support::cli_command()
            .env("NO_COLOR", "1")
            .args([
                "run",
                "--parallel",
                "1",
                "--progress",
                "none",
                "--log-format",
                format,
                "--log-output",
                out_path.to_str().unwrap(),
                dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "--log-format {format} must exit 0 on an all-passing suite\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
mod scrub_tests {
    use super::*;

    // Regression: on Windows `tmp_root` is a raw OS path (single `\`), but
    // json/allure report bodies escape every `\` as `\\` — the exact-string
    // replace never matched and the raw path leaked into the golden
    // comparison.
    #[test]
    fn scrub_matches_a_json_escaped_backslash_path_against_a_raw_tmp_root() {
        let tmp_root = Path::new("C:\\Users\\foo\\AppData\\Local\\Temp\\.tmpABC");
        let text = "\"fullName\": \"C:\\\\Users\\\\foo\\\\AppData\\\\Local\\\\Temp\\\\.tmpABC\\\\a_pass.gctf\"";
        assert_eq!(
            scrub(text, tmp_root),
            "\"fullName\": \"<TMPDIR>/a_pass.gctf\""
        );
    }

    // Regression: the html reporter percent-encodes a real `/` as `&#x2f;`
    // but never touches `\` (Windows-only), so on Windows the join separator
    // between `<TMPDIR>` and the filename survives as a literal `\` even
    // though the Unix-generated golden always carries the escaped form.
    #[test]
    fn scrub_normalizes_a_raw_backslash_join_to_html_escaped_slash_in_html_output() {
        let tmp_root = Path::new("C:\\Users\\foo\\AppData\\Local\\Temp\\.tmpABC");
        let text = "<span class=\"test-name\">C:\\Users\\foo\\AppData\\Local\\Temp\\.tmpABC\\a_pass.gctf</span> grpc.health.v1.Health&#x2f;Check";
        assert_eq!(
            scrub(text, tmp_root),
            "<span class=\"test-name\"><TMPDIR>&#x2f;a_pass.gctf</span> grpc.health.v1.Health&#x2f;Check"
        );
    }

    // Regression: allure carries the version as a name/value pair, which the
    // `"version":"…"` pattern never reached, so the golden pinned a literal
    // version and broke on every release bump.
    #[test]
    fn scrub_replaces_the_allure_version_label_and_parameter() {
        let v = env!("CARGO_PKG_VERSION");
        let text = format!(
            r#"{{"name":"grpctestify_version","value":"{v}"}},{{"name":"grpctestify.version","value":"{v}"}}"#
        );
        assert_eq!(
            scrub(&text, Path::new("/tmp/AbC123")),
            r#"{"name":"grpctestify_version","value":"<VERSION>"},{"name":"grpctestify.version","value":"<VERSION>"}"#
        );
    }

    #[test]
    fn scrub_matches_an_unescaped_unix_style_path_directly() {
        let tmp_root = Path::new("/tmp/AbC123");
        let text = "\"fullName\": \"/tmp/AbC123/a_pass.gctf\"";
        assert_eq!(
            scrub(text, tmp_root),
            "\"fullName\": \"<TMPDIR>/a_pass.gctf\""
        );
    }
}
