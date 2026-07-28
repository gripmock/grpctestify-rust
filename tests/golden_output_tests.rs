#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Golden/snapshot coverage for every `run` output form. Non-deterministic
//! content (timestamps, durations, temp paths, uuids, hostnames) is scrubbed
//! to stable placeholders and compared against `tests/golden/`.
//! `--parallel 1` avoids scheduling-order/timing-tie flakiness.
//! Regenerate: `UPDATE_GOLDEN=1 cargo test --test golden_output_tests`

#[path = "support/mod.rs"]
mod support;

use std::path::Path;

async fn spawn_health_server() -> String {
    let (reporter, health_service) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("build reflection service");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(reflection_service)
            .serve_with_incoming(incoming)
            .await
            .expect("health server run");
    });

    addr.to_string()
}

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
    // filename wasn't part of `tmp_root` itself — normalize it (single `\`,
    // or JSON-escaped-doubled `\\`) to the golden files' Unix-style
    // `<TMPDIR>/...`.
    s = s.replace("<TMPDIR>\\\\", "<TMPDIR>/");
    s = s.replace("<TMPDIR>\\", "<TMPDIR>/");

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

fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/output_forms")
        .join(format!("{name}.golden"))
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var("DEBUG_GOLDEN").is_ok() {
        std::fs::write(format!("/tmp/{name}.actual"), actual).unwrap();
    }
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden file {}: {e}\nrun with UPDATE_GOLDEN=1 to create it\nactual output was:\n{actual}",
            path.display()
        )
    });
    assert_eq!(
        expected, actual,
        "golden mismatch for '{name}' (rerun with UPDATE_GOLDEN=1 if this change is intentional)"
    );
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
