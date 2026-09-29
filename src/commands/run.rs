use anyhow::Result;
use futures::stream::StreamExt;
use std::collections::{BTreeSet, HashMap};
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

enum WorkItem {
    File(PathBuf),
    Row {
        doc: Arc<GctfDocument>,
        vars: HashMap<String, serde_json::Value>,
        name: String,
    },
    Error {
        name: String,
        message: String,
    },
}

impl WorkItem {
    fn display_name(&self) -> String {
        match self {
            WorkItem::File(path) => path.to_string_lossy().to_string(),
            WorkItem::Row { name, .. } | WorkItem::Error { name, .. } => name.clone(),
        }
    }
}

fn stringify_row_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn format_row_name(
    file: &str,
    index: usize,
    vars: &HashMap<String, serde_json::Value>,
) -> String {
    let mut keys: Vec<&String> = vars.keys().collect();
    keys.sort();
    let fields = keys
        .iter()
        .map(|k| format!("{}={}", k, stringify_row_value(&vars[*k])))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}#[row={} {}]", file, index, fields)
}

fn row_params(vars: &HashMap<String, serde_json::Value>) -> Vec<(String, String)> {
    let mut keys: Vec<&String> = vars.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| (k.clone(), stringify_row_value(&vars[k])))
        .collect()
}

pub(crate) fn collect_data_rows(
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
        indexed_by: None,
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

pub(crate) fn environment_address(dir: &std::path::Path) -> Option<String> {
    crate::serve::project::address_of(&crate::serve::project::project_variables(dir))
}

pub(crate) fn dataset_row_vars(row: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    match row {
        serde_json::Value::Object(fields) => fields
            .iter()
            .map(|(k, v)| (format!("dataset.{k}"), v.clone()))
            .collect(),
        _ => HashMap::new(),
    }
}

fn expand_dataset_files(
    files: Vec<PathBuf>,
    write: bool,
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<WorkItem>) {
    let mut plain = Vec::new();
    let mut dataset_files = Vec::new();
    let mut items = Vec::new();

    for file in files {
        let doc = match parser::parse_gctf(&file) {
            Ok(d) => d,
            Err(_) => {
                plain.push(file);
                continue;
            }
        };
        let Some(section) = doc.first_section(SectionType::Dataset) else {
            plain.push(file);
            continue;
        };
        let rows = match &section.content {
            SectionContent::Rows(rows) => rows.clone(),
            SectionContent::Empty => Vec::new(),
            _ => {
                plain.push(file);
                continue;
            }
        };

        let file_str = file.to_string_lossy().to_string();
        dataset_files.push(file.clone());

        if write {
            items.push(WorkItem::Error {
                name: file_str,
                message: "--write is not supported with a DATASET section (parameterized) run"
                    .to_string(),
            });
            continue;
        }
        if rows.is_empty() {
            items.push(WorkItem::Error {
                name: file_str,
                message: "DATASET section has zero rows".to_string(),
            });
            continue;
        }

        let doc = Arc::new(doc);
        for (i, row) in rows.iter().enumerate() {
            let vars = dataset_row_vars(row);
            let name = format_row_name(&file_str, i, &vars);
            items.push(WorkItem::Row {
                doc: doc.clone(),
                vars,
                name,
            });
        }
    }

    (plain, dataset_files, items)
}

pub(crate) fn extract_test_meta(doc: &parser::ast::GctfDocument) -> TestMeta {
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

pub(crate) fn tags_match(
    file_tags: &[String],
    tags_include: &[String],
    skip_tags: &[String],
) -> bool {
    for tag in tags_include {
        if !file_tags.iter().any(|t| t == tag) {
            return false;
        }
    }
    !(!skip_tags.is_empty() && file_tags.iter().any(|t| skip_tags.contains(t)))
}

fn file_matches_meta(
    path: &Path,
    tags_include: &[String],
    skip_tags: &[String],
    noticed: &mut Vec<String>,
) -> bool {
    let parse_result = parser::parse_with_recovery(path);
    for diagnostic in &parse_result.diagnostics.diagnostics {
        if diagnostic.message.contains("Invalid META") {
            let said = format!(
                "{}: {} — its tags are not what --tags reads",
                path.display(),
                diagnostic.message
            );
            warn!("{said}");
            noticed.push(said);
        }
    }
    let file_tags = extract_test_meta(&parse_result.document).tags;
    tags_match(&file_tags, tags_include, skip_tags)
}

fn is_retryable_grpc_code(code: u32) -> bool {
    matches!(code, 4 | 14)
}

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

fn should_retry_message(message: &str) -> bool {
    extract_transport_grpc_code(message).is_some_and(is_retryable_grpc_code)
}

fn is_retryable_http_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

fn the_status_is_what_failed(result: &execution::TestExecutionResult) -> bool {
    result
        .assertions
        .iter()
        .any(|a| !a.passed && a.expression.contains("@status("))
}

fn http_answer_is_worth_dialling_again(result: &execution::TestExecutionResult) -> bool {
    result.http_status.is_some_and(is_retryable_http_status) && the_status_is_what_failed(result)
}

pub(crate) fn should_retry_result(result: &execution::TestExecutionResult) -> bool {
    match &result.status {
        execution::TestExecutionStatus::Pass => false,
        execution::TestExecutionStatus::Fail(msg) => {
            if result.failure_kind == Some(execution::FailureKind::Transport) {
                return should_retry_message(msg) || extract_transport_grpc_code(msg).is_none();
            }
            http_answer_is_worth_dialling_again(result)
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DirFixtures {
    pub(crate) setup: Option<PathBuf>,
    pub(crate) teardown: Option<PathBuf>,
}

pub(crate) fn partition_fixtures(
    files: Vec<PathBuf>,
) -> (Vec<PathBuf>, HashMap<PathBuf, DirFixtures>) {
    let mut tests = Vec::new();
    let mut fixtures: HashMap<PathBuf, DirFixtures> = HashMap::new();
    for file in files {
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        match fixture_role(&file) {
            Some(FixtureRole::Setup) => fixtures.entry(dir).or_default().setup = Some(file),
            Some(FixtureRole::Teardown) => fixtures.entry(dir).or_default().teardown = Some(file),
            None => tests.push(file),
        }
    }
    (tests, fixtures)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FixtureRole {
    Setup,
    Teardown,
}

pub(crate) fn fixture_role(file: &Path) -> Option<FixtureRole> {
    let known = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ["gctf", "httf", "apif"].contains(&ext.to_ascii_lowercase().as_str()));
    if !known {
        return None;
    }
    match file.file_stem().and_then(|n| n.to_str()) {
        Some("_setup") => Some(FixtureRole::Setup),
        Some("_teardown") => Some(FixtureRole::Teardown),
        _ => None,
    }
}

fn work_item_dir(item: &WorkItem) -> Option<PathBuf> {
    match item {
        WorkItem::File(path) => path.parent().map(Path::to_path_buf),
        WorkItem::Row { doc, .. } => Path::new(&doc.file_path).parent().map(Path::to_path_buf),
        WorkItem::Error { .. } => None,
    }
}

fn item_skipped_by_setup<'a>(
    item_dir: Option<&Path>,
    dirs_setup_failed: &'a HashMap<PathBuf, String>,
) -> Option<&'a str> {
    item_dir
        .and_then(|d| dirs_setup_failed.get(d))
        .map(String::as_str)
}

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

fn should_capture_exchange(
    explicit: bool,
    verbose_console: bool,
    format_uses_exchange: bool,
    has_log_output: bool,
) -> bool {
    explicit || verbose_console || (format_uses_exchange && has_log_output)
}

fn report_output_path(base: &Path, format: crate::cli::LogFormat, multiple: bool) -> PathBuf {
    if !multiple {
        return base.to_path_buf();
    }
    let file_name = match format {
        crate::cli::LogFormat::Json => "json.json",
        crate::cli::LogFormat::Yaml => "yaml.yaml",
        crate::cli::LogFormat::JUnit => "junit.xml",
        crate::cli::LogFormat::Html => "html.html",
        crate::cli::LogFormat::Allure => "allure",
        crate::cli::LogFormat::Console => "console",
    };
    base.join(file_name)
}

pub async fn run_tests(cli: &Cli, args: &RunArgs) -> Result<()> {
    crate::parser::register_extra_inline_option_keys(
        crate::plugins::rhai_plugin::load_all_inline_option_keys(),
    );

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
    let mut missing = Vec::new();
    for path in &args.test_paths {
        if path.is_dir() {
            collected.extend(FileUtils::collect_test_files(path, exclude_patterns));
        } else if path.is_file() {
            collected.push(path.clone());
        } else {
            missing.push(path.display().to_string());
        }
    }
    if !missing.is_empty() {
        anyhow::bail!("test path not found: {}", missing.join(", "));
    }

    let (mut test_files, fixtures) = partition_fixtures(collected);

    let mut nothing_changed = false;
    if args.only_changed {
        match crate::only_changed::changed_files(
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            &args.since,
            &test_files,
        ) {
            Ok(changed) => {
                let before = test_files.len();
                test_files.retain(|f| changed.contains(f));
                info!(
                    "--only-changed: {} of {before} test file(s) changed since '{}'",
                    test_files.len(),
                    args.since
                );
                nothing_changed = before > 0 && test_files.is_empty();
            }
            Err(e) => {
                warn!("--only-changed: {e:#} — running all files instead");
            }
        }
    }

    let mut noticed: Vec<String> = Vec::new();
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

        test_files.retain(|path| file_matches_meta(path, &tags_inc, &tags_exc, &mut noticed));

        info!("Filtered to {} test file(s) by META", test_files.len());
    }

    info!("Found {} test file(s)", test_files.len());

    if nothing_changed && test_files.is_empty() {
        println!(
            "Nothing to run — no test file changed since '{}'.",
            args.since
        );
        return Ok(());
    }

    if test_files.is_empty() {
        use crate::report::style::{warn_icon, warn_style};
        eprintln!(
            "{} {}",
            warn_icon(),
            warn_style().apply_to("No test files found (paths or tag filters matched nothing)")
        );
        std::process::exit(1);
    }

    FileUtils::sort_files(&mut test_files, &args.sort);

    let active_dirs: BTreeSet<PathBuf> = test_files
        .iter()
        .filter_map(|f| f.parent().map(Path::to_path_buf))
        .collect();
    let fixture_count: usize = active_dirs
        .iter()
        .filter_map(|d| fixtures.get(d))
        .map(|fx| usize::from(fx.setup.is_some()) + usize::from(fx.teardown.is_some()))
        .sum();

    let (test_files, dataset_files, dataset_work_items) =
        expand_dataset_files(test_files, args.write);
    if args.data.is_some() && !dataset_files.is_empty() {
        anyhow::bail!(
            "--data cannot be combined with a DATASET section (found in: {}) — pick one row source per run",
            dataset_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let mut work_items: Vec<WorkItem> = match &args.data {
        Some(data) => {
            expand_templates_over_data(test_files, data, args.data_format.as_deref(), args.write)
        }
        None => test_files.into_iter().map(WorkItem::File).collect(),
    };
    work_items.extend(dataset_work_items);
    let total_work = work_items.len();
    let total_reported = total_work + fixture_count;

    if args.stream {
    } else {
        let noun = if total_work == 1 { "test" } else { "tests" };
        let workers = if total_work > 1 && parallel_jobs > 1 {
            report::style::dim_style()
                .apply_to(format!(" · {parallel_jobs} workers"))
                .to_string()
        } else {
            String::new()
        };
        println!(
            "{} {total_work} {noun}{workers}",
            report::style::bold_style().apply_to("Running")
        );
    }

    let mut reporters: Vec<Box<dyn report::Reporter>> = Vec::new();

    let target_address = std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
        .ok()
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(config::default_address);
    let address_shown = match std::env::var(config::ENV_GRPCTESTIFY_ADDRESS) {
        Ok(from_env) if !from_env.trim().is_empty() => {
            format!("{from_env} (from $GRPCTESTIFY_ADDRESS; a file's own ADDRESS wins)")
        }
        _ if !work_items.is_empty()
            && work_items.iter().all(|item| match item {
                WorkItem::File(path) => {
                    crate::parser::ast::Family::of(&path.to_string_lossy())
                        == crate::parser::ast::Family::Httf
                }
                WorkItem::Row { doc, .. } => {
                    crate::parser::ast::Family::of(&doc.file_path)
                        == crate::parser::ast::Family::Httf
                }
                WorkItem::Error { .. } => true,
            }) =>
        {
            "named by each file (an HTTP call has no default target)".to_string()
        }
        _ => format!(
            "{} (the gRPC default, where a file names none)",
            config::default_address()
        ),
    };

    let env_info = report::console::EnvironmentInfo {
        address: address_shown,
        parallel_jobs,
        sort_mode: args.sort.clone(),
        dry_run: args.dry_run,
        warnings: noticed.clone(),
    };

    if args.stream {
        reporters.push(Box::new(report::StreamingJsonReporter::new(total_reported)));
    } else {
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

    let requested_formats = cli.log_format_modes();
    if !requested_formats.is_empty() {
        if let Some(output_path) = &args.log_output {
            let multiple = requested_formats.len() > 1;
            if multiple && let Err(e) = std::fs::create_dir_all(output_path) {
                warn!(
                    "Failed to create --log-output directory {}: {e}",
                    output_path.display()
                );
            }

            for format in &requested_formats {
                let path = report_output_path(output_path, *format, multiple);
                match format {
                    crate::cli::LogFormat::Json => {
                        reporters.push(Box::new(report::JsonReporter::new(path)));
                    }
                    crate::cli::LogFormat::JUnit => {
                        reporters.push(Box::new(report::JunitReporter::new(path)));
                    }
                    crate::cli::LogFormat::Allure => {
                        reporters.push(Box::new(
                            report::AllureReporter::new(path).with_address(target_address.clone()),
                        ));
                    }
                    crate::cli::LogFormat::Yaml => {
                        reporters.push(Box::new(report::YamlReporter::new(path)));
                    }
                    crate::cli::LogFormat::Html => {
                        reporters.push(Box::new(report::HtmlReporter::new(path)));
                    }
                    crate::cli::LogFormat::Console => {}
                }
            }
        } else {
            warn!(
                "--log-format specified but --log-output is missing. File report will be skipped."
            );
        }
    }

    for reporter in report::load_all_configured_reporters() {
        reporters.push(reporter);
    }

    let mut test_results = TestResults::new();

    let coverage_collector = if args.coverage {
        Some(Arc::new(report::CoverageCollector::new()))
    } else {
        None
    };

    let verbose_console = matches!(cli.progress_mode(), crate::cli::args::ProgressMode::Verbose);
    let format_uses_exchange = cli.log_format_modes().iter().any(|f| {
        matches!(
            f,
            crate::cli::LogFormat::Allure
                | crate::cli::LogFormat::Json
                | crate::cli::LogFormat::Yaml
                | crate::cli::LogFormat::Html
                | crate::cli::LogFormat::JUnit
        )
    });
    let capture_exchange = should_capture_exchange(
        args.capture_exchange,
        verbose_console,
        format_uses_exchange,
        args.log_output.is_some(),
    );

    let start_time = std::time::Instant::now();
    let runner = Arc::new({
        let runner = execution::TestRunner::new(
            args.dry_run,
            args.timeout,
            args.no_assert,
            args.write,
            cli.verbose,
            coverage_collector.clone(),
        )
        .with_protocol(args.protocol.parse().unwrap_or_default())
        .with_capture_exchange(capture_exchange);
        match environment_address(&std::env::current_dir().unwrap_or_default()) {
            Some(address) => runner.with_env_address(address),
            None => runner,
        }
    });

    let reporters: Arc<Vec<Box<dyn report::Reporter>>> = Arc::new(reporters);

    let project_env: HashMap<String, serde_json::Value> =
        crate::serve::project::project_variables(&std::env::current_dir().unwrap_or_default())
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();

    let mut dir_setup_vars: HashMap<PathBuf, HashMap<String, serde_json::Value>> = HashMap::new();
    let mut dirs_setup_failed: HashMap<PathBuf, String> = HashMap::new();
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
            let named = setup
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "_setup".to_string());
            dirs_setup_failed.insert(dir.clone(), named);
        }
    }

    let dir_setup_vars = Arc::new(dir_setup_vars);
    let dirs_setup_failed = Arc::new(dirs_setup_failed);

    let stream = futures::stream::iter(work_items)
        .map(|item| {
            let runner = runner.clone();
            let reporters = reporters.clone();
            let dir_setup_vars = dir_setup_vars.clone();
            let dirs_setup_failed = dirs_setup_failed.clone();
            let project_env = project_env.clone();
            let name = item.display_name();

            async move {
                for r in reporters.iter() {
                    r.on_test_start(&name);
                }

                let test_start = std::time::Instant::now();
                let item_dir = work_item_dir(&item);

                let mut test_result = if let Some(fixture) =
                    item_skipped_by_setup(item_dir.as_deref(), &dirs_setup_failed)
                {
                    let mut result = TestResult::pass(name.clone(), 0, None);
                    result.status = crate::state::TestStatus::Skip;
                    result.error_message = Some(format!(
                        "Skipped: directory setup fixture ({fixture}) failed"
                    ));
                    result
                } else {
                    let mut initial_vars = project_env.clone();
                    if let Some(vars) = item_dir.as_ref().and_then(|d| dir_setup_vars.get(d)) {
                        initial_vars.extend(vars.clone());
                    }
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
                            let params = row_params(&vars);
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
                                Ok(res) => execution_result_to_test_result(name, res)
                                    .with_row_params(params),
                                Err(e) => TestResult::fail(
                                    name,
                                    format!("Execution error: {}", e),
                                    0,
                                    None,
                                )
                                .with_row_params(params),
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
        } else if args.is_html_coverage() {
            println!("{}", collector.generate_html_report());
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

fn execution_result_to_test_result(
    name: String,
    res: execution::TestExecutionResult,
) -> TestResult {
    let call_duration = res.call_duration_ms;
    let meta = res.meta;
    let config_summary = res.config_summary;
    let assertions = res.assertions;
    let retried = res.retried;
    let document_durations_ms = res.document_durations_ms;
    let exchange = res.captured_response.map(|resp| {
        crate::state::CapturedExchange::capture(resp.headers, resp.trailers, resp.messages)
    });
    match res.status {
        execution::TestExecutionStatus::Pass => {
            TestResult::pass_with_meta(name, 0, call_duration, meta)
                .with_assertions(assertions)
                .with_exchange(exchange)
                .with_retried(retried)
                .with_document_durations(document_durations_ms)
                .with_config_summary(config_summary)
        }
        execution::TestExecutionStatus::Fail(msg) => {
            TestResult::fail_with_meta(name, msg, 0, call_duration, meta)
                .with_assertions(assertions)
                .with_exchange(exchange)
                .with_retried(retried)
                .with_document_durations(document_durations_ms)
                .with_config_summary(config_summary)
        }
    }
}

async fn run_template_row(
    runner: &execution::TestRunner,
    doc: &GctfDocument,
    vars: HashMap<String, serde_json::Value>,
    retry: u32,
    retry_delay: f64,
    no_retry: bool,
) -> Result<execution::TestExecutionResult> {
    let test_meta = extract_test_meta(doc);
    let config_summary = apif_state::ConfigSummary::from_document(doc);

    if let Err(e) = parser::validate_document_chain(doc) {
        return Ok(
            execution::TestExecutionResult::fail(format!("Validation error: {}", e), None)
                .with_meta(test_meta)
                .with_config_summary(config_summary),
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
            .with_meta(test_meta)
            .with_config_summary(config_summary));
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

    Ok(result
        .with_meta(test_meta)
        .with_config_summary(config_summary))
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

    let test_meta = extract_test_meta(&doc);
    let config_summary = apif_state::ConfigSummary::from_document(&doc);

    if let Err(e) = parser::validate_document_chain(&doc) {
        return Ok(
            execution::TestExecutionResult::fail(format!("Validation error: {}", e), None)
                .with_meta(test_meta)
                .with_config_summary(config_summary),
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
            .with_meta(test_meta)
            .with_config_summary(config_summary));
        }
    };

    let max_retries = if effective_runtime.no_retry.value {
        0
    } else {
        effective_runtime.retry.value
    };

    let mut attempt = 0u32;
    let result = loop {
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

    if runner.is_write_mode()
        && let Some(resp) = &result.captured_response
        && let Err(e) = crate::utils::file::update_test_file(file, &doc, resp)
    {
        return Ok(execution::TestExecutionResult::fail(
            format!("Failed to update test file: {}", e),
            result.call_duration_ms,
        )
        .with_meta(test_meta)
        .with_config_summary(config_summary));
    }

    Ok(result
        .with_meta(test_meta)
        .with_config_summary(config_summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{
        GctfAttribute, GctfDocument, Section, SectionContent, SectionSpan, SectionType,
    };

    #[test]
    fn should_capture_exchange_explicit_flag_forces_it() {
        assert!(should_capture_exchange(true, false, false, false));
    }

    #[test]
    fn should_capture_exchange_default_paths_unchanged() {
        assert!(!should_capture_exchange(false, false, false, false));
        assert!(should_capture_exchange(false, true, false, false));
        assert!(!should_capture_exchange(false, false, true, false));
        assert!(should_capture_exchange(false, false, true, true));
    }

    #[test]
    fn report_output_path_single_format_keeps_exact_path() {
        let base = PathBuf::from("report.xml");
        assert_eq!(
            report_output_path(&base, crate::cli::LogFormat::JUnit, false),
            base
        );
    }

    #[test]
    fn report_output_path_multiple_formats_join_under_base() {
        let base = PathBuf::from("out");
        assert_eq!(
            report_output_path(&base, crate::cli::LogFormat::JUnit, true),
            base.join("junit.xml")
        );
        assert_eq!(
            report_output_path(&base, crate::cli::LogFormat::Html, true),
            base.join("html.html")
        );
        assert_eq!(
            report_output_path(&base, crate::cli::LogFormat::Allure, true),
            base.join("allure")
        );
    }

    #[test]
    fn retry_only_on_retryable_transport_status() {
        assert!(should_retry_message(
            "Validation failed:\n  - Failed to start gRPC stream: gRPC error code=14 message=connection refused"
        ));
        assert!(should_retry_message(
            "gRPC error: code=4 message=deadline exceeded"
        ));
    }

    #[test]
    fn no_retry_on_non_retryable_transport_status() {
        assert!(!should_retry_message(
            "gRPC error: code=5 message=not found"
        ));
        assert!(!should_retry_message(
            "gRPC error: code=3 message=invalid argument"
        ));
    }

    #[test]
    fn retry_result_requires_transport_kind() {
        let assertion = execution::TestExecutionResult::fail(
            "Validation failed:\n  - gRPC error code=14 message=unavailable".to_string(),
            None,
        );
        assert_eq!(
            assertion.failure_kind,
            Some(execution::FailureKind::Assertion)
        );
        assert!(!should_retry_result(&assertion));

        let transport = execution::TestExecutionResult::fail(
            "Validation failed:\n  - Failed to start gRPC stream: gRPC error code=14 message=unavailable".to_string(),
            None,
        )
        .with_failure_kind(execution::FailureKind::Transport);
        assert!(should_retry_result(&transport));

        let transport_terminal = execution::TestExecutionResult::fail(
            "gRPC error code=5 message=not found".to_string(),
            None,
        )
        .with_failure_kind(execution::FailureKind::Transport);
        assert!(!should_retry_result(&transport_terminal));

        assert!(!should_retry_result(&execution::TestExecutionResult::pass(
            None
        )));
    }

    #[test]
    fn an_http_answer_that_did_not_arrive_is_retried_like_unavailable() {
        let unreachable = execution::TestExecutionResult::fail(
            "Could not reach http://127.0.0.1:1/health: connection refused".to_string(),
            None,
        )
        .with_failure_kind(execution::FailureKind::Transport);
        assert!(should_retry_result(&unreachable));
    }

    fn with_assertion(
        result: execution::TestExecutionResult,
        expression: &str,
        passed: bool,
    ) -> execution::TestExecutionResult {
        let mut result = result;
        result.assertions.push(apif_state::AssertionRecord {
            line: 1,
            expression: expression.to_string(),
            passed,
            elapsed_ms: 0,
            message: None,
            endpoint: None,
            expected: None,
            actual: None,
            hint: None,
        });
        result
    }

    #[test]
    fn a_grpc_failure_is_not_retried_because_some_step_answered_503() {
        let chain = execution::TestExecutionResult::fail(
            "Validation failed:\n  - .status == \"SERVING\": got \"NOT_SERVING\"".to_string(),
            Some(3),
        )
        .with_http_status(503);
        assert!(
            !should_retry_result(&chain),
            "an assertion failure is settled, whatever status some other step carried"
        );
    }

    #[test]
    fn only_a_failed_status_assertion_makes_a_transient_answer_worth_dialling_again() {
        let expected = with_assertion(
            execution::TestExecutionResult::fail(
                "Validation failed:\n  - .error == \"slow down\"".to_string(),
                Some(3),
            )
            .with_http_status(429),
            "@status() == 429",
            true,
        );
        assert!(
            !should_retry_result(&expected),
            "the test asked for the 429 and failed on the body — retrying changes nothing"
        );

        let unexpected = with_assertion(
            execution::TestExecutionResult::fail(
                "Validation failed:\n  - @status() == 200: got 429".to_string(),
                Some(3),
            )
            .with_http_status(429),
            "@status() == 200",
            false,
        );
        assert!(should_retry_result(&unexpected));
    }

    #[test]
    fn an_http_5xx_or_429_that_failed_the_test_is_retried_but_a_4xx_is_not() {
        for status in [429u16, 502, 503, 504] {
            let overloaded = with_assertion(
                execution::TestExecutionResult::fail(
                    format!("Validation failed:\n  - @status() == 200: got {status}"),
                    Some(3),
                )
                .with_http_status(status),
                "@status() == 200",
                false,
            );
            assert!(should_retry_result(&overloaded), "{status}");
        }
        for status in [400u16, 404, 500, 200] {
            let settled = with_assertion(
                execution::TestExecutionResult::fail(
                    format!("Validation failed:\n  - @status() == 201: got {status}"),
                    Some(3),
                )
                .with_http_status(status),
                "@status() == 201",
                false,
            );
            assert!(!should_retry_result(&settled), "{status}");
        }
        let expected = execution::TestExecutionResult::pass(Some(3)).with_http_status(503);
        assert!(!should_retry_result(&expected));
    }

    #[test]
    fn a_body_check_that_failed_on_a_503_is_settled_not_retried() {
        let body_only = execution::TestExecutionResult::fail(
            "Validation failed:\n  - Response mismatch at line 7".to_string(),
            Some(3),
        )
        .with_http_status(503);
        assert!(
            !should_retry_result(&body_only),
            "only a status the test asked for and did not get is worth dialling again"
        );
    }

    #[test]
    fn assertion_failure_with_timeout_text_is_not_retried() {
        assert!(!should_retry_message(
            "Validation failed:\n  - Error mismatch at line 12:\n  - expected \"request timeout exceeded\", got \"ok\""
        ));
        assert!(!should_retry_message(
            "Validation failed:\n  - expected error message to contain 'network unavailable'"
        ));
        assert!(!should_retry_message(
            "Validation failed:\n  - expected {\"code\": 14} got {\"code\": 0}"
        ));
    }

    #[test]
    fn extract_test_meta_from_file_meta() {
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
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.name, Some("suite name".to_string()));
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
        assert_eq!(meta.owner, Some("team-a".to_string()));
        assert_eq!(meta.summary, Some("test summary".to_string()));
    }

    #[test]
    fn extract_test_meta_fallback_tags_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("tag", "smoke,integration")],
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(
            meta.tags,
            vec!["smoke".to_string(), "integration".to_string()]
        );
    }

    #[test]
    fn extract_test_meta_no_fallback_when_meta_has_tags() {
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
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 3,
            end_line: 4,
            attributes: vec![GctfAttribute::new("tag", "integration")],
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
    }

    #[test]
    fn extract_test_meta_fallback_owner_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("owner", "team-b")],
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.owner, Some("team-b".to_string()));
    }

    #[test]
    fn extract_test_meta_fallback_summary_from_attributes() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("summary", "quick test")],
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.summary, Some("quick test".to_string()));
    }

    #[test]
    fn extract_test_meta_dedup_tags() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 1,
            end_line: 2,
            attributes: vec![GctfAttribute::new("tag", "smoke")],
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Empty,
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 3,
            end_line: 4,
            attributes: vec![GctfAttribute::new("tag", "smoke")],
            span: SectionSpan::default(),
        });

        let meta = extract_test_meta(&doc);
        assert_eq!(meta.tags, vec!["smoke".to_string()]);
    }

    #[test]
    fn extract_test_meta_empty() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let meta = extract_test_meta(&doc);
        assert!(meta.is_empty());
    }

    const TEMPLATE_GCTF: &str = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- REQUEST ---\n{ \"user\": \"{{users.user}}\" }\n\n--- RESPONSE ---\n{ \"role\": \"{{users.role}}\" }\n";

    #[test]
    fn per_row_failure_fails_the_suite_but_keeps_all_results() {
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

    const DATASET_GCTF: &str = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- DATASET ---\n- id: '1'\n  name: Ada\n- id: '2'\n  name: Grace\n\n--- REQUEST ---\n{ \"id\": \"{{dataset.id}}\" }\n\n--- RESPONSE ---\n{ \"name\": \"{{dataset.name}}\" }\n";

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_dataset_files_yields_one_row_per_entry() {
        let dir = std::env::temp_dir().join("gctf_run_dataset_expand_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gctf = dir.join("template.gctf");
        std::fs::write(&gctf, DATASET_GCTF).unwrap();

        let (plain, dataset_files, items) = expand_dataset_files(vec![gctf.clone()], false);
        assert!(plain.is_empty());
        assert_eq!(dataset_files, vec![gctf]);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|it| matches!(it, WorkItem::Row { .. })));

        let ada = items
            .iter()
            .find_map(|it| match it {
                WorkItem::Row { vars, name, .. } if name.contains("dataset.id=1") => {
                    let _ = name;
                    Some(vars)
                }
                _ => None,
            })
            .expect("row for id=1");
        assert_eq!(ada.get("dataset.name"), Some(&serde_json::json!("Ada")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_dataset_files_passes_through_files_without_a_dataset_section() {
        let (plain, dataset_files, items) =
            expand_dataset_files(vec![PathBuf::from("plain.gctf")], false);
        assert_eq!(plain, vec![PathBuf::from("plain.gctf")]);
        assert!(dataset_files.is_empty());
        assert!(items.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_dataset_files_rejects_write() {
        let dir = std::env::temp_dir().join("gctf_run_dataset_write_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gctf = dir.join("template.gctf");
        std::fs::write(&gctf, DATASET_GCTF).unwrap();

        let (_, _, items) = expand_dataset_files(vec![gctf], true);
        assert_eq!(items.len(), 1);
        match &items[0] {
            WorkItem::Error { message, .. } => assert!(message.contains("--write")),
            _ => panic!("expected --write rejection"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn expand_dataset_files_rejects_zero_rows() {
        let dir = std::env::temp_dir().join("gctf_run_dataset_empty_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gctf = dir.join("template.gctf");
        let content = "--- ENDPOINT ---\nsvc.Svc/Call\n\n--- DATASET ---\n[]\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n";
        std::fs::write(&gctf, content).unwrap();

        let (_, _, items) = expand_dataset_files(vec![gctf], false);
        assert_eq!(items.len(), 1);
        match &items[0] {
            WorkItem::Error { message, .. } => assert!(message.contains("zero rows")),
            _ => panic!("expected zero-rows rejection"),
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
    fn a_directory_of_http_tests_has_fixtures_too() {
        let files = vec![
            PathBuf::from("api/_setup.httf"),
            PathBuf::from("api/list.httf"),
            PathBuf::from("api/_teardown.httf"),
        ];
        let (tests, fixtures) = partition_fixtures(files);

        assert_eq!(tests, vec![PathBuf::from("api/list.httf")]);
        let fx = &fixtures[&PathBuf::from("api")];
        assert_eq!(fx.setup, Some(PathBuf::from("api/_setup.httf")));
        assert_eq!(fx.teardown, Some(PathBuf::from("api/_teardown.httf")));
    }

    #[test]
    fn only_a_test_file_can_be_a_fixture() {
        assert_eq!(
            fixture_role(Path::new("d/_setup.gctf")),
            Some(FixtureRole::Setup)
        );
        assert_eq!(
            fixture_role(Path::new("d/_setup.httf")),
            Some(FixtureRole::Setup)
        );
        assert_eq!(
            fixture_role(Path::new("d/_teardown.httf")),
            Some(FixtureRole::Teardown)
        );
        assert_eq!(fixture_role(Path::new("d/_setup.md")), None);
        assert_eq!(fixture_role(Path::new("d/_setup")), None);
        assert_eq!(fixture_role(Path::new("d/setup.httf")), None);
    }

    #[test]
    fn partition_fixtures_excludes_and_groups_by_dir() {
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
        let mut failed = HashMap::new();
        failed.insert(PathBuf::from("suite"), "_setup.httf".to_string());

        assert_eq!(
            item_skipped_by_setup(Some(Path::new("suite")), &failed),
            Some("_setup.httf")
        );
        assert_eq!(
            item_skipped_by_setup(Some(Path::new("other")), &failed),
            None
        );
        assert_eq!(item_skipped_by_setup(None, &failed), None);
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
