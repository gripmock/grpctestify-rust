// Run command - execute tests

use anyhow::Result;
use futures::stream::StreamExt;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

use crate::cli::Cli;
use crate::cli::args::RunArgs;
use crate::config;
use crate::execution;
use crate::parser;
use crate::parser::ast::{GctfDocument, SectionContent, SectionType};
use crate::report;
use crate::state::{TestMeta, TestResult, TestResults};
use crate::utils::FileUtils;

/// A single unit of work fed through the parallel execution stream.
///
/// Without `--data`, files map 1:1 to a [`WorkItem::File`]. With `--data`, each
/// target file is treated as a template and expanded up-front into one
/// [`WorkItem::Row`] per data row (all sharing the parsed template document), or
/// a single [`WorkItem::Error`] when the source is empty/unreadable.
enum WorkItem {
    /// Ordinary file: parsed and executed as today.
    File(PathBuf),
    /// One parameterized row: shared template + row variables.
    Row {
        doc: Arc<GctfDocument>,
        vars: HashMap<String, serde_json::Value>,
        name: String,
    },
    /// A pre-determined failure (empty source, bad source, `--write` with `--data`).
    Error { name: String, message: String },
}

impl WorkItem {
    fn display_name(&self) -> String {
        match self {
            WorkItem::File(path) => path.to_string_lossy().to_string(),
            WorkItem::Row { name, .. } | WorkItem::Error { name, .. } => name.clone(),
        }
    }
}

/// Render a row's variables into a stable, human-readable identity suffix so
/// each expanded case reports distinctly: `<file>#[row=<i> k=v k=v]`.
fn format_row_name(file: &str, index: usize, vars: &HashMap<String, serde_json::Value>) -> String {
    let mut keys: Vec<&String> = vars.keys().collect();
    keys.sort();
    let fields = keys
        .iter()
        .map(|k| {
            let rendered = match &vars[*k] {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{}={}", k, rendered)
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}#[row={} {}]", file, index, fields)
}

/// Read every row of a `--data` source into template variables.
///
/// The source is fed through the same `SourceDrivenConfig` data plane used by
/// `bench`, so each row's columns arrive namespaced under the source name
/// (`<source>.<column>`). The source path is resolved against the current
/// working directory (absolutised) so it is independent of any template's
/// location. `format` overrides the extension-inferred source format.
fn collect_data_rows(
    data: &Path,
    format: Option<crate::bench::sources::SourceFormat>,
) -> Result<Vec<HashMap<String, serde_json::Value>>> {
    let abs = std::path::absolute(data).unwrap_or_else(|_| data.to_path_buf());
    let name = data
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "data".to_string());

    let def = crate::bench::sources::SourceDefinition {
        file: abs.to_string_lossy().to_string(),
        name: Some(name),
        format,
        delimiter: None,
        header: None,
        indexed_by: None,
        index_mode: None,
        memory_budget: None,
        filter: None,
        join_type: None,
    };

    let Some(config) = crate::bench::sources::SourceDrivenConfig::prepare(&[def], data)? else {
        return Ok(Vec::new());
    };

    let mut rows = Vec::new();
    while let Some(vars) = config.next_row_variables()? {
        rows.push(vars);
    }
    Ok(rows)
}

/// Expand every template file across the rows of a `--data` source.
///
/// Each file becomes one [`WorkItem::Row`] per row (sharing the parsed template
/// document). An empty source, an unreadable source, or a `--write` request all
/// resolve to a single failing item per file so CI cannot silently pass.
fn expand_templates_over_data(
    files: Vec<PathBuf>,
    data: &Path,
    data_format: Option<&str>,
    write: bool,
) -> Vec<WorkItem> {
    let per_file_error = |files: Vec<PathBuf>, message: String| -> Vec<WorkItem> {
        files
            .into_iter()
            .map(|f| WorkItem::Error {
                name: f.to_string_lossy().to_string(),
                message: message.clone(),
            })
            .collect()
    };

    // `--write` snapshots a response back into the template; with N rows the
    // target is ambiguous, so we reject it rather than silently pick a row.
    if write {
        return per_file_error(
            files,
            "--write is not supported with --data (parameterized) runs".to_string(),
        );
    }

    let format = match data_format {
        Some(f) => match f.parse::<crate::bench::sources::SourceFormat>() {
            Ok(fmt) => Some(fmt),
            Err(_) => {
                return per_file_error(
                    files,
                    format!("invalid --data-format '{f}' (expected csv, tsv, or ndjson)"),
                );
            }
        },
        None => None,
    };

    let rows = match collect_data_rows(data, format) {
        Ok(rows) => rows,
        Err(e) => return per_file_error(files, format!("--data error: {e}")),
    };

    if rows.is_empty() {
        return per_file_error(
            files,
            format!("--data source produced zero rows: {}", data.display()),
        );
    }

    let mut items = Vec::new();
    for file in files {
        let doc = match parser::parse_gctf(&file) {
            Ok(d) => Arc::new(d),
            // Let the normal file path surface the parse error once.
            Err(_) => {
                items.push(WorkItem::File(file));
                continue;
            }
        };
        let file_str = file.to_string_lossy().to_string();
        for (i, vars) in rows.iter().enumerate() {
            let name = format_row_name(&file_str, i, vars);
            items.push(WorkItem::Row {
                doc: doc.clone(),
                vars: vars.clone(),
                name,
            });
        }
    }
    items
}

fn extract_test_meta(doc: &parser::ast::GctfDocument) -> TestMeta {
    let mut meta = doc
        .sections
        .iter()
        .find_map(|s: &parser::ast::Section| {
            if let SectionContent::Meta(m) = &s.content
                && s.section_type == SectionType::Meta
            {
                return Some(TestMeta::from_file_meta(m));
            }
            None
        })
        .unwrap_or_default();

    if meta.tags.is_empty() {
        for section in &doc.sections {
            if let Some(tag_attr) = section.get_attribute("tag") {
                for t in tag_attr.value.split(',') {
                    let trimmed = t.trim();
                    if !trimmed.is_empty() && !meta.tags.contains(&trimmed.to_string()) {
                        meta.tags.push(trimmed.to_string());
                    }
                }
            }
        }
    }
    if meta.owner.is_none() {
        for section in &doc.sections {
            if let Some(owner_attr) = section.get_attribute("owner") {
                meta.owner = Some(owner_attr.value.clone());
                break;
            }
        }
    }
    if meta.summary.is_none() {
        for section in &doc.sections {
            if let Some(summary_attr) = section.get_attribute("summary") {
                meta.summary = Some(summary_attr.value.clone());
                break;
            }
        }
    }

    meta
}

fn file_matches_meta(path: &Path, tags_include: &[String], skip_tags: &[String]) -> bool {
    let parse_result = parser::parse_with_recovery(path);
    let doc = parse_result.document;

    let meta = doc.sections.iter().find_map(|s: &parser::ast::Section| {
        if let SectionContent::Meta(m) = &s.content
            && s.section_type == SectionType::Meta
        {
            Some(m)
        } else {
            None
        }
    });

    let file_tags: Vec<&str> = meta
        .map(|m| m.tags.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    for tag in tags_include {
        if !file_tags.iter().any(|t| t == tag) {
            return false;
        }
    }

    if !skip_tags.is_empty() && file_tags.iter().any(|t| skip_tags.iter().any(|e| t == e)) {
        return false;
    }

    true
}

/// gRPC status codes that indicate a transient transport/availability failure
/// worth retrying. Application-level failures and, crucially, assertion
/// mismatches are never retryable.
fn is_retryable_grpc_code(code: u32) -> bool {
    matches!(
        code,
        4  // DEADLINE_EXCEEDED
        | 14 // UNAVAILABLE
    )
}

/// Extract a gRPC status code from a failure message, but only when it is part
/// of the canonical transport-error token (`gRPC error[:] code=<N>`). This
/// deliberately avoids substring matching of arbitrary text: an assertion
/// failure whose expected/actual payload merely contains the word "timeout" or
/// a JSON `"code": N` field carries no such token and is therefore never
/// classified as retryable.
fn extract_transport_grpc_code(message: &str) -> Option<u32> {
    let marker = message.find("gRPC error")?;
    let after = &message[marker..];
    let code_pos = after.find("code=")?;
    let digits: String = after[code_pos + "code=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok()
}

/// Decide whether a failed test should be retried.
///
/// Retry is driven by the actual gRPC transport status code, not by loose
/// substring matching. Assertion/validation failures (which never carry a
/// retryable transport status) are never retried.
fn should_retry_message(message: &str) -> bool {
    extract_transport_grpc_code(message).is_some_and(is_retryable_grpc_code)
}

/// Decide whether a completed attempt should be retried. Retries are gated on
/// BOTH the structured failure kind (transport-level) AND a retryable gRPC
/// status code. The `failure_kind` guard structurally guarantees that assertion
/// and validation failures are never retried, independent of their message text.
fn should_retry_result(result: &execution::TestExecutionResult) -> bool {
    match &result.status {
        execution::TestExecutionStatus::Pass => false,
        execution::TestExecutionStatus::Fail(msg) => {
            result.failure_kind == Some(execution::FailureKind::Transport)
                && should_retry_message(msg)
        }
    }
}

/// Per-directory FIXTURES discovered by convention. `_setup.gctf` runs once
/// before a directory's tests (seeding their initial variables); `_teardown.gctf`
/// runs once after, always. They live in the normal `.gctf` glob, so they are
/// split out of the test set here rather than executed as ordinary tests.
#[derive(Debug, Default)]
struct DirFixtures {
    setup: Option<PathBuf>,
    teardown: Option<PathBuf>,
}

/// Split collected `.gctf` files into ordinary tests and per-directory fixtures.
///
/// `_setup.gctf`/`_teardown.gctf` are pulled out of the test set entirely and
/// grouped by their parent directory (exact-directory scope: a dir's fixtures
/// apply only to tests in that same dir, never to nested subdirectories).
fn partition_fixtures(files: Vec<PathBuf>) -> (Vec<PathBuf>, HashMap<PathBuf, DirFixtures>) {
    let mut tests = Vec::new();
    let mut fixtures: HashMap<PathBuf, DirFixtures> = HashMap::new();
    for file in files {
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        match file.file_name().and_then(|n| n.to_str()) {
            Some("_setup.gctf") => fixtures.entry(dir).or_default().setup = Some(file),
            Some("_teardown.gctf") => fixtures.entry(dir).or_default().teardown = Some(file),
            _ => tests.push(file),
        }
    }
    (tests, fixtures)
}

/// Directory a work item belongs to, used to look up its fixtures. `Error` items
/// (pre-determined failures) have no directory context.
fn work_item_dir(item: &WorkItem) -> Option<PathBuf> {
    match item {
        WorkItem::File(path) => path.parent().map(Path::to_path_buf),
        WorkItem::Row { doc, .. } => Path::new(&doc.file_path).parent().map(Path::to_path_buf),
        WorkItem::Error { .. } => None,
    }
}

/// A test is skipped (never executed) when its directory's setup fixture failed;
/// its teardown still runs (unconditionally, in `run_tests`).
fn item_skipped_by_setup(item_dir: Option<&Path>, dirs_setup_failed: &HashSet<PathBuf>) -> bool {
    item_dir.is_some_and(|d| dirs_setup_failed.contains(d))
}

/// Run one fixture file as its own reportable unit, mirroring the reporter
/// lifecycle of a normal test. Returns `(passed, captured_vars, result)`: the
/// captured vars are the fixture's EXTRACT bindings (empty for teardown or on
/// any failure) that seed the directory's tests.
async fn run_fixture(
    runner: &execution::TestRunner,
    file: &Path,
    reporters: &[Box<dyn report::Reporter>],
) -> (bool, HashMap<String, serde_json::Value>, TestResult) {
    let name = file.to_string_lossy().to_string();
    for r in reporters.iter() {
        r.on_test_start(&name);
    }
    let start = std::time::Instant::now();

    let (passed, vars, mut result) = match parser::parse_gctf(file) {
        Err(e) => (
            false,
            HashMap::new(),
            TestResult::fail(name.clone(), format!("Parse error: {}", e), 0, None),
        ),
        Ok(doc) => {
            if let Err(e) = parser::validate_document_chain(&doc) {
                (
                    false,
                    HashMap::new(),
                    TestResult::fail(name.clone(), format!("Validation error: {}", e), 0, None),
                )
            } else {
                match runner.run_test_capturing_vars(&doc).await {
                    Err(e) => (
                        false,
                        HashMap::new(),
                        TestResult::fail(name.clone(), format!("Execution error: {}", e), 0, None),
                    ),
                    Ok((res, vars)) => match res.status {
                        execution::TestExecutionStatus::Pass => (
                            true,
                            vars,
                            TestResult::pass(name.clone(), 0, res.call_duration_ms),
                        ),
                        execution::TestExecutionStatus::Fail(msg) => (
                            false,
                            HashMap::new(),
                            TestResult::fail(name.clone(), msg, 0, res.call_duration_ms),
                        ),
                    },
                }
            }
        }
    };

    result.duration_ms = start.elapsed().as_millis() as u64;
    for r in reporters.iter() {
        r.on_test_end(&name, &result);
    }
    (passed, vars, result)
}

pub async fn run_tests(cli: &Cli, args: &RunArgs) -> Result<()> {
    // Defensive clamp: buffer_unordered(0) never polls and deadlocks the run.
    let parallel_jobs = cli.parallel_jobs().max(1);
    info!("Parallel jobs: {}", parallel_jobs);

    if args.dry_run {
        info!("Dry-run mode enabled");
    }

    if args.no_assert {
        info!("No-assert mode enabled (skipping assertions)");
    }

    let mut collected = Vec::new();
    let exclude_patterns = &args.exclude;
    for path in &args.test_paths {
        if path.is_dir() {
            collected.extend(FileUtils::collect_test_files(path, exclude_patterns));
        } else if path.is_file() {
            collected.push(path.clone());
        }
    }

    // Convention-based per-directory fixtures (`_setup.gctf`/`_teardown.gctf`)
    // are pulled out of the normal test set before any tag filtering so they are
    // never executed as ordinary tests.
    let (mut test_files, fixtures) = partition_fixtures(collected);

    let has_meta_filters = !args.tags.is_empty() || !args.skip_tags.is_empty();

    if has_meta_filters {
        let tags_inc: Vec<String> = args
            .tags
            .iter()
            .flat_map(|t| t.split(','))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let tags_exc: Vec<String> = args
            .skip_tags
            .iter()
            .flat_map(|t| t.split(','))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        test_files.retain(|path| file_matches_meta(path, &tags_inc, &tags_exc));

        info!("Filtered to {} test file(s) by META", test_files.len());
    }

    info!("Found {} test file(s)", test_files.len());

    if test_files.is_empty() {
        // An empty (or fully filtered) test set is almost always a mistake
        // (typo in path or --tags); exit non-zero so CI cannot silently pass.
        warn!("No test files found");
        eprintln!(
            "⚠️  WARN [{}]: No test files found (paths or tag filters matched nothing)",
            chrono::Local::now().format("%H:%M:%S")
        );
        std::process::exit(1);
    }

    FileUtils::sort_files(&mut test_files, &args.sort);

    // Directories with surviving (post-filter) tests. A dir whose tests were all
    // filtered out is inactive: its setup+teardown are skipped entirely.
    let active_dirs: BTreeSet<PathBuf> = test_files
        .iter()
        .filter_map(|f| f.parent().map(Path::to_path_buf))
        .collect();
    // Setup+teardown fixtures that will actually run, so the reporter total
    // accounts for them (they are surfaced as their own results).
    let fixture_count: usize = active_dirs
        .iter()
        .filter_map(|d| fixtures.get(d))
        .map(|fx| usize::from(fx.setup.is_some()) + usize::from(fx.teardown.is_some()))
        .sum();

    // With --data, expand each template file into one work item per data row.
    // Without it, files pass through 1:1. META filtering already ran above (per
    // file), so every row of a template inherits its file's tags.
    let work_items: Vec<WorkItem> = match &args.data {
        Some(data) => {
            expand_templates_over_data(test_files, data, args.data_format.as_deref(), args.write)
        }
        None => test_files.into_iter().map(WorkItem::File).collect(),
    };
    let total_work = work_items.len();
    let total_reported = total_work + fixture_count;

    if args.stream {
        // Silent mode - streaming output only
    } else if total_work == 1 {
        println!(
            "ℹ️  INFO [{}]: Running 1 test sequentially...",
            chrono::Local::now().format("%H:%M:%S")
        );
    } else if parallel_jobs <= 1 {
        println!(
            "ℹ️  INFO [{}]: Running {} test(s) sequentially...",
            chrono::Local::now().format("%H:%M:%S"),
            total_work
        );
    } else {
        println!(
            "ℹ️  INFO [{}]: Running {} test(s) in parallel (jobs: {})...",
            chrono::Local::now().format("%H:%M:%S"),
            total_work,
            parallel_jobs
        );
    }

    let mut reporters: Vec<Box<dyn report::Reporter>> = Vec::new();

    let env_info = report::console::EnvironmentInfo {
        address: std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
            .unwrap_or_else(|_| config::default_address()),
        parallel_jobs,
        sort_mode: args.sort.clone(),
        dry_run: args.dry_run,
    };

    if args.stream {
        reporters.push(Box::new(report::StreamingJsonReporter::new(total_reported)));
    } else {
        // Always add console reporter (unless streaming)
        let mode = match cli.progress_mode() {
            crate::cli::args::ProgressMode::Dots => report::ConsoleMode::Dots,
            crate::cli::args::ProgressMode::Verbose => report::ConsoleMode::Verbose,
            crate::cli::args::ProgressMode::None => report::ConsoleMode::Silent,
        };
        reporters.push(Box::new(report::ConsoleReporter::new(
            mode,
            total_reported as u64,
            env_info,
        )));
    }

    if let Some(format) = cli.log_format_mode() {
        if let Some(output_path) = &args.log_output {
            match format {
                crate::cli::LogFormat::Json => {
                    reporters.push(Box::new(report::JsonReporter::new(output_path.clone())));
                }
                crate::cli::LogFormat::JUnit => {
                    reporters.push(Box::new(report::JunitReporter::new(output_path.clone())));
                }
                crate::cli::LogFormat::Allure => {
                    reporters.push(Box::new(report::AllureReporter::new(output_path.clone())));
                }
                crate::cli::LogFormat::Yaml => {
                    reporters.push(Box::new(report::YamlReporter::new(output_path.clone())));
                }
                _ => {}
            }
        } else {
            warn!(
                "--log-format specified but --log-output is missing. File report will be skipped."
            );
        }
    }

    let mut test_results = TestResults::new();

    let coverage_collector = if args.coverage {
        Some(Arc::new(report::CoverageCollector::new()))
    } else {
        None
    };

    let start_time = std::time::Instant::now();
    let runner = Arc::new(
        execution::TestRunner::new(
            args.dry_run,
            args.timeout,
            args.no_assert,
            args.write,
            cli.verbose,
            coverage_collector.clone(),
        )
        .with_protocol(args.protocol.parse().unwrap_or_default()),
    );

    let reporters: Arc<Vec<Box<dyn report::Reporter>>> = Arc::new(reporters);

    // Setup barrier: run each active directory's `_setup.gctf` sequentially
    // before its tests, capturing its EXTRACT bindings as that dir's initial
    // variables. A setup failure is recorded (its dependent tests are skipped)
    // but is surfaced as its own result and never suppresses teardown.
    let mut dir_setup_vars: HashMap<PathBuf, HashMap<String, serde_json::Value>> = HashMap::new();
    let mut dirs_setup_failed: HashSet<PathBuf> = HashSet::new();
    let mut fixture_results: Vec<TestResult> = Vec::new();

    for dir in &active_dirs {
        let Some(setup) = fixtures.get(dir).and_then(|fx| fx.setup.as_ref()) else {
            continue;
        };
        let (passed, vars, result) = run_fixture(&runner, setup, &reporters).await;
        fixture_results.push(result);
        if passed {
            dir_setup_vars.insert(dir.clone(), vars);
        } else {
            dirs_setup_failed.insert(dir.clone());
        }
    }

    let dir_setup_vars = Arc::new(dir_setup_vars);
    let dirs_setup_failed = Arc::new(dirs_setup_failed);

    // Use a stream for bounded parallelism
    let stream = futures::stream::iter(work_items)
        .map(|item| {
            let runner = runner.clone();
            let reporters = reporters.clone();
            let dir_setup_vars = dir_setup_vars.clone();
            let dirs_setup_failed = dirs_setup_failed.clone();
            let name = item.display_name();

            async move {
                for r in reporters.iter() {
                    r.on_test_start(&name);
                }

                let test_start = std::time::Instant::now();
                let item_dir = work_item_dir(&item);

                let mut test_result =
                    if item_skipped_by_setup(item_dir.as_deref(), &dirs_setup_failed) {
                        TestResult::fail(
                            name.clone(),
                            "Skipped: directory setup fixture (_setup.gctf) failed".to_string(),
                            0,
                            None,
                        )
                    } else {
                        // Tests in a dir with a passing setup are seeded with its
                        // captured variables; dirs without setup get an empty map,
                        // which is byte-for-byte the normal `run_test` path.
                        let initial_vars = item_dir
                            .as_ref()
                            .and_then(|d| dir_setup_vars.get(d))
                            .cloned()
                            .unwrap_or_default();
                        match item {
                            WorkItem::File(file) => {
                                let file_path_str = file.to_string_lossy().to_string();
                                match run_single_test(
                                    &runner,
                                    &file,
                                    initial_vars,
                                    args.retry,
                                    args.retry_delay,
                                    args.no_retry,
                                )
                                .await
                                {
                                    Ok(res) => execution_result_to_test_result(file_path_str, res),
                                    Err(e) => TestResult::fail(
                                        file_path_str,
                                        format!("Execution error: {}", e),
                                        0,
                                        None,
                                    ),
                                }
                            }
                            WorkItem::Row { doc, vars, name } => {
                                // Fixtures + `--data`: setup vars seed the row, but
                                // row vars win on key conflicts (row identity is
                                // explicit and must not be overwritten by a fixture).
                                let mut merged = initial_vars;
                                merged.extend(vars);
                                match run_template_row(
                                    &runner,
                                    &doc,
                                    merged,
                                    args.retry,
                                    args.retry_delay,
                                    args.no_retry,
                                )
                                .await
                                {
                                    Ok(res) => execution_result_to_test_result(name, res),
                                    Err(e) => TestResult::fail(
                                        name,
                                        format!("Execution error: {}", e),
                                        0,
                                        None,
                                    ),
                                }
                            }
                            WorkItem::Error { name, message } => {
                                TestResult::fail(name, message, 0, None)
                            }
                        }
                    };

                test_result.duration_ms = test_start.elapsed().as_millis() as u64;

                for r in reporters.iter() {
                    r.on_test_end(&name, &test_result);
                }

                test_result
            }
        })
        .buffer_unordered(parallel_jobs);

    let results: Vec<TestResult> = stream.collect().await;

    // Teardown barrier: run each active directory's `_teardown.gctf` after its
    // tests drain, ALWAYS — whether the tests or the setup passed or failed. A
    // teardown failure is a distinct failing unit and is reported as such.
    for dir in &active_dirs {
        let Some(teardown) = fixtures.get(dir).and_then(|fx| fx.teardown.as_ref()) else {
            continue;
        };
        let (_passed, _vars, result) = run_fixture(&runner, teardown, &reporters).await;
        fixture_results.push(result);
    }

    for result in results {
        test_results.add(result);
    }

    for result in fixture_results {
        test_results.add(result);
    }

    let total_duration = start_time.elapsed().as_millis() as u64;
    test_results.metrics.total_duration_ms = total_duration;
    test_results.metrics.parallel_jobs = parallel_jobs;

    for r in reporters.iter() {
        r.on_suite_end(&test_results)?;
    }

    if let Some(collector) = coverage_collector {
        if args.is_json_coverage() {
            let report = collector.generate_json_report();
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            let report = collector.generate_text_report();
            if !args.stream {
                println!("\n{}", report);
            }
        }
    }

    if !test_results.all_passed() {
        std::process::exit(1);
    }

    Ok(())
}

/// Map a completed execution result onto a reportable [`TestResult`] with the
/// given identity `name` (a file path, or a per-row identity for table cases).
fn execution_result_to_test_result(
    name: String,
    res: execution::TestExecutionResult,
) -> TestResult {
    let call_duration = res.call_duration_ms;
    let meta = res.meta;
    match res.status {
        execution::TestExecutionStatus::Pass => {
            TestResult::pass_with_meta(name, 0, call_duration, meta)
        }
        execution::TestExecutionStatus::Fail(msg) => {
            TestResult::fail_with_meta(name, msg, 0, call_duration, meta)
        }
    }
}

/// Execute a single parameterized row against a shared template document.
///
/// Mirrors [`run_single_test`] (validation + retry loop) but runs the already
/// parsed `doc` with `vars` via the existing `run_test_with_variables`. No
/// `--write` handling: template runs reject `--write` during expansion.
async fn run_template_row(
    runner: &execution::TestRunner,
    doc: &GctfDocument,
    vars: HashMap<String, serde_json::Value>,
    retry: u32,
    retry_delay: f64,
    no_retry: bool,
) -> Result<execution::TestExecutionResult> {
    let test_meta = extract_test_meta(doc);

    if let Err(e) = parser::validate_document_chain(doc) {
        return Ok(
            execution::TestExecutionResult::fail(format!("Validation error: {}", e), None)
                .with_meta(test_meta),
        );
    }

    let effective_runtime = match execution::runner_helpers::resolve_effective_runtime_options(
        doc,
        execution::runner_helpers::CliRuntimeDefaults {
            timeout_seconds: 30,
            retry,
            retry_delay_seconds: retry_delay,
            no_retry,
        },
    ) {
        Ok(v) => v,
        Err(e) => {
            return Ok(execution::TestExecutionResult::fail(
                format!("Validation error: {}", e),
                None,
            )
            .with_meta(test_meta));
        }
    };

    let max_retries = if effective_runtime.no_retry.value {
        0
    } else {
        effective_runtime.retry.value
    };

    let mut attempt = 0u32;
    let result = loop {
        let current = runner.run_test_with_variables(doc, vars.clone()).await?;

        if !should_retry_result(&current) || attempt >= max_retries {
            break current;
        }

        attempt += 1;
        if effective_runtime.retry_delay_seconds.value > 0.0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(
                effective_runtime.retry_delay_seconds.value,
            ))
            .await;
        }
    };

    Ok(result.with_meta(test_meta))
}

async fn run_single_test(
    runner: &execution::TestRunner,
    file: &std::path::Path,
    initial_vars: HashMap<String, serde_json::Value>,
    retry: u32,
    retry_delay: f64,
    no_retry: bool,
) -> Result<execution::TestExecutionResult> {
    let doc = match parser::parse_gctf(file) {
        Ok(d) => d,
        Err(e) => {
            return Ok(execution::TestExecutionResult::fail(
                format!("Parse error: {}", e),
                None,
            ));
        }
    };

    // Extract META for reports
    let test_meta = extract_test_meta(&doc);

    if let Err(e) = parser::validate_document_chain(&doc) {
        return Ok(
            execution::TestExecutionResult::fail(format!("Validation error: {}", e), None)
                .with_meta(test_meta),
        );
    }

    let effective_runtime = match execution::runner_helpers::resolve_effective_runtime_options(
        &doc,
        execution::runner_helpers::CliRuntimeDefaults {
            timeout_seconds: 30,
            retry,
            retry_delay_seconds: retry_delay,
            no_retry,
        },
    ) {
        Ok(v) => v,
        Err(e) => {
            return Ok(execution::TestExecutionResult::fail(
                format!("Validation error: {}", e),
                None,
            )
            .with_meta(test_meta));
        }
    };

    let max_retries = if effective_runtime.no_retry.value {
        0
    } else {
        effective_runtime.retry.value
    };

    let mut attempt = 0u32;
    let result = loop {
        // An empty `initial_vars` map makes this identical to `run_test`; a
        // non-empty one seeds the chain with a directory's `_setup.gctf` vars.
        let current = runner
            .run_test_with_variables(&doc, initial_vars.clone())
            .await?;

        let should_retry = should_retry_result(&current);

        if !should_retry || attempt >= max_retries {
            break current;
        }

        attempt += 1;
        if effective_runtime.retry_delay_seconds.value > 0.0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(
                effective_runtime.retry_delay_seconds.value,
            ))
            .await;
        }
    };

    if let Some(resp) = &result.captured_response
        && let Err(e) = crate::utils::file::update_test_file(file, &doc, resp)
    {
        return Ok(execution::TestExecutionResult::fail(
            format!("Failed to update test file: {}", e),
            result.call_duration_ms,
        )
        .with_meta(test_meta));
    }

    Ok(result.with_meta(test_meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{GctfAttribute, GctfDocument, Section, SectionContent, SectionType};

    #[test]
    fn retry_only_on_retryable_transport_status() {
        // Genuine transport failures carry the canonical gRPC error token.
        assert!(should_retry_message(
            "Validation failed:\n  - Failed to start gRPC stream: gRPC error code=14 message=connection refused"
        ));
        assert!(should_retry_message(
            "gRPC error: code=4 message=deadline exceeded"
        ));
    }

    #[test]
    fn no_retry_on_non_retryable_transport_status() {
        // NOT_FOUND / INVALID_ARGUMENT and friends are terminal.
        assert!(!should_retry_message(
            "gRPC error: code=5 message=not found"
        ));
        assert!(!should_retry_message(
            "gRPC error: code=3 message=invalid argument"
        ));
    }

    #[test]
    fn retry_result_requires_transport_kind() {
        // A retryable gRPC code but classified as an Assertion failure → no retry.
        let assertion = execution::TestExecutionResult::fail(
            "Validation failed:\n  - gRPC error code=14 message=unavailable".to_string(),
            None,
        );
        assert_eq!(
            assertion.failure_kind,
            Some(execution::FailureKind::Assertion)
        );
        assert!(!should_retry_result(&assertion));

        // Same message, but a genuine transport failure → retry.
        let transport = execution::TestExecutionResult::fail(
            "Validation failed:\n  - Failed to start gRPC stream: gRPC error code=14 message=unavailable".to_string(),
            None,
        )
        .with_failure_kind(execution::FailureKind::Transport);
        assert!(should_retry_result(&transport));

        // Transport failure with a terminal code → no retry.
        let transport_terminal = execution::TestExecutionResult::fail(
            "gRPC error code=5 message=not found".to_string(),
            None,
        )
        .with_failure_kind(execution::FailureKind::Transport);
        assert!(!should_retry_result(&transport_terminal));

        // Passing result is never retried.
        assert!(!should_retry_result(&execution::TestExecutionResult::pass(
            None
        )));
    }

    #[test]
    fn assertion_failure_with_timeout_text_is_not_retried() {
        // Regression: an assertion mismatch whose expected text merely contains
        // the word "timeout" (or "network"/"unavailable") must never be retried.
        assert!(!should_retry_message(
            "Validation failed:\n  - Error mismatch at line 12:\n  - expected \"request timeout exceeded\", got \"ok\""
        ));
        assert!(!should_retry_message(
            "Validation failed:\n  - expected error message to contain 'network unavailable'"
        ));
        // A JSON `\"code\": 14` field in an assertion payload is not a transport token.
        assert!(!should_retry_message(
            "Validation failed:\n  - expected {\"code\": 14} got {\"code\": 0}"
        ));
    }

    #[test]
    fn test_extract_test_meta_from_file_meta() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let file_meta = crate::parser::ast::FileMeta {
            name: Some("suite name".to_string()),
            tags: vec!["smoke".to_string()],
            owner: Some("team-a".to_string()),
            summary: Some("test summary".to_string()),
            links: vec![],
        };
        doc.sections.push(Section {
            section_type: SectionType::Meta,
            content: SectionContent::Meta(file_meta),
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 5,
            attributes: vec![],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.name, Some("suite name".to_string()));
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
        assert_eq!(meta.owner, Some("team-a".to_string()));
        assert_eq!(meta.summary, Some("test summary".to_string()));
    }

    #[test]
    fn test_extract_test_meta_fallback_tags_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("tag", "smoke,integration")],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(
            meta.tags,
            vec!["smoke".to_string(), "integration".to_string()]
        );
    }

    #[test]
    fn test_extract_test_meta_no_fallback_when_meta_has_tags() {
        let file_meta = crate::parser::ast::FileMeta {
            tags: vec!["smoke".to_string()],
            ..Default::default()
        };
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Meta,
            content: SectionContent::Meta(file_meta),
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![],
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 3,
            end_line: 4,
            attributes: vec![GctfAttribute::new("tag", "integration")],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
    }

    #[test]
    fn test_extract_test_meta_fallback_owner_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("owner", "team-b")],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.owner, Some("team-b".to_string()));
    }

    #[test]
    fn test_extract_test_meta_fallback_summary_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("summary", "quick test")],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.summary, Some("quick test".to_string()));
    }

    #[test]
    fn test_extract_test_meta_dedup_tags() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("tag", "smoke")],
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 3,
            end_line: 4,
            attributes: vec![GctfAttribute::new("tag", "smoke")],
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
    }

    #[test]
    fn test_extract_test_meta_empty() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let meta = extract_test_meta(&doc);
        assert!(meta.is_empty());
    }

    const TEMPLATE_GCTF: &str = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- REQUEST ---\n{ \"user\": \"{{users.user}}\" }\n\n--- RESPONSE ---\n{ \"role\": \"{{users.role}}\" }\n";

    #[test]
    fn per_row_failure_fails_the_suite_but_keeps_all_results() {
        // Rows are independent stream items: a passing row and a failing row
        // both report, and any failure fails the suite.
        let mut results = TestResults::new();
        results.add(TestResult::pass("t.gctf#[row=0 users.user=alice]", 0, None));
        results.add(TestResult::fail(
            "t.gctf#[row=1 users.user=bob]",
            "assertion failed".to_string(),
            0,
            None,
        ));
        assert_eq!(results.total(), 2);
        assert!(!results.all_passed());
    }

    #[test]
    fn expand_rejects_write_with_data() {
        let items = expand_templates_over_data(
            vec![PathBuf::from("t.gctf")],
            Path::new("users.csv"),
            None,
            true,
        );
        assert_eq!(items.len(), 1);
        match &items[0] {
            WorkItem::Error { message, .. } => assert!(message.contains("--write")),
            _ => panic!("expected --write rejection"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_over_csv_yields_one_item_per_row() {
        let dir = std::env::temp_dir().join("gctf_run_data_expand_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("users.csv"), "user,role\nalice,admin\nbob,guest\n").unwrap();
        let gctf = dir.join("template.gctf");
        std::fs::write(&gctf, TEMPLATE_GCTF).unwrap();

        let items =
            expand_templates_over_data(vec![gctf.clone()], &dir.join("users.csv"), None, false);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|it| matches!(it, WorkItem::Row { .. })));

        let names: Vec<String> = items.iter().map(WorkItem::display_name).collect();
        assert_ne!(names[0], names[1], "row identities must be distinct");
        assert!(names.iter().any(|n| n.contains("users.user=alice")));
        assert!(names.iter().any(|n| n.contains("users.user=bob")));

        // Per-row variables are namespaced under the source name and carried through.
        let alice = items
            .iter()
            .find_map(|it| match it {
                WorkItem::Row { vars, name, .. } if name.contains("users.user=alice") => Some(vars),
                _ => None,
            })
            .expect("alice row");
        assert_eq!(
            alice.get("users.role"),
            Some(&serde_json::json!("admin".to_string()))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_empty_source_is_a_failure() {
        let dir = std::env::temp_dir().join("gctf_run_data_empty_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Header only — zero data rows.
        std::fs::write(dir.join("users.csv"), "user,role\n").unwrap();
        let gctf = dir.join("template.gctf");
        std::fs::write(&gctf, TEMPLATE_GCTF).unwrap();

        let items = expand_templates_over_data(vec![gctf], &dir.join("users.csv"), None, false);
        assert_eq!(items.len(), 1);
        match &items[0] {
            WorkItem::Error { message, .. } => assert!(message.contains("zero rows")),
            _ => panic!("expected zero-row failure"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expand_rejects_bad_data_format() {
        let items = expand_templates_over_data(
            vec![PathBuf::from("t.gctf")],
            Path::new("users.dat"),
            Some("xlsx"),
            false,
        );
        assert_eq!(items.len(), 1);
        match &items[0] {
            WorkItem::Error { message, .. } => assert!(message.contains("invalid --data-format")),
            _ => panic!("expected format rejection"),
        }
    }

    #[test]
    fn partition_fixtures_excludes_and_groups_by_dir() {
        // `_setup.gctf`/`_teardown.gctf` must never land in the normal test set;
        // they are grouped under their exact parent directory.
        let files = vec![
            PathBuf::from("suite/a.gctf"),
            PathBuf::from("suite/_setup.gctf"),
            PathBuf::from("suite/_teardown.gctf"),
            PathBuf::from("suite/b.gctf"),
            PathBuf::from("suite/nested/c.gctf"),
            PathBuf::from("suite/nested/_setup.gctf"),
        ];
        let (tests, fixtures) = partition_fixtures(files);

        assert_eq!(
            tests,
            vec![
                PathBuf::from("suite/a.gctf"),
                PathBuf::from("suite/b.gctf"),
                PathBuf::from("suite/nested/c.gctf"),
            ]
        );
        assert!(!tests.iter().any(|t| t.to_string_lossy().contains("_setup")));
        assert!(
            !tests
                .iter()
                .any(|t| t.to_string_lossy().contains("_teardown"))
        );

        let suite = fixtures.get(Path::new("suite")).expect("suite fixtures");
        assert_eq!(suite.setup, Some(PathBuf::from("suite/_setup.gctf")));
        assert_eq!(suite.teardown, Some(PathBuf::from("suite/_teardown.gctf")));

        // Exact-directory scope: nested setup belongs only to the nested dir.
        let nested = fixtures
            .get(Path::new("suite/nested"))
            .expect("nested fixtures");
        assert_eq!(
            nested.setup,
            Some(PathBuf::from("suite/nested/_setup.gctf"))
        );
        assert_eq!(nested.teardown, None);
    }

    #[test]
    fn partition_fixtures_no_fixtures_is_passthrough() {
        // No fixture files → tests pass through unchanged and no dir is tracked.
        let files = vec![PathBuf::from("a.gctf"), PathBuf::from("dir/b.gctf")];
        let (tests, fixtures) = partition_fixtures(files.clone());
        assert_eq!(tests, files);
        assert!(fixtures.is_empty());
    }

    #[test]
    fn work_item_dir_resolves_parent() {
        assert_eq!(
            work_item_dir(&WorkItem::File(PathBuf::from("suite/a.gctf"))),
            Some(PathBuf::from("suite"))
        );
        let err = WorkItem::Error {
            name: "x".to_string(),
            message: "boom".to_string(),
        };
        assert_eq!(work_item_dir(&err), None);
    }

    #[test]
    fn setup_failure_skips_only_dependent_dir_tests() {
        // Lifecycle decision: a test is skipped iff its directory's setup failed.
        // Tests in other dirs (and dirless items) are unaffected; teardown is
        // driven separately and always runs (see `run_tests`).
        let mut failed = HashSet::new();
        failed.insert(PathBuf::from("suite"));

        assert!(item_skipped_by_setup(Some(Path::new("suite")), &failed));
        assert!(!item_skipped_by_setup(Some(Path::new("other")), &failed));
        assert!(!item_skipped_by_setup(None, &failed));
    }

    #[test]
    fn parse_run_data_flags() {
        use clap::Parser;
        let cli = crate::cli::Cli::parse_from([
            "grpctestify",
            "run",
            "tests/",
            "--data",
            "rows.csv",
            "--data-format",
            "csv",
        ]);
        let args = cli.get_run_args();
        assert_eq!(args.data, Some(PathBuf::from("rows.csv")));
        assert_eq!(args.data_format.as_deref(), Some("csv"));
    }
}
