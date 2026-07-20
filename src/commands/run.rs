// Run command - execute tests

use anyhow::Result;
use futures::stream::StreamExt;
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

use crate::cli::Cli;
use crate::cli::args::RunArgs;
use crate::config;
use crate::execution;
use crate::parser;
use crate::parser::ast::{SectionContent, SectionType};
use crate::report;
use crate::state::{TestMeta, TestResult, TestResults};
use crate::utils::FileUtils;

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

pub async fn run_tests(cli: &Cli, args: &RunArgs) -> Result<()> {
    // Get parallel job count
    // Defensive clamp: buffer_unordered(0) never polls and deadlocks the run.
    let parallel_jobs = cli.parallel_jobs().max(1);
    info!("Parallel jobs: {}", parallel_jobs);

    // Handle dry-run mode
    if args.dry_run {
        info!("Dry-run mode enabled");
    }

    if args.no_assert {
        info!("No-assert mode enabled (skipping assertions)");
    }

    // Collect test files
    let mut test_files = Vec::new();
    let exclude_patterns = &args.exclude;
    for path in &args.test_paths {
        if path.is_dir() {
            test_files.extend(FileUtils::collect_test_files(path, exclude_patterns));
        } else if path.is_file() {
            test_files.push(path.clone());
        }
    }

    // Filter by META tags if provided
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

    // Sort files
    FileUtils::sort_files(&mut test_files, &args.sort);

    if args.stream {
        // Silent mode - streaming output only
    } else if test_files.len() == 1 {
        println!(
            "ℹ️  INFO [{}]: Running 1 test sequentially...",
            chrono::Local::now().format("%H:%M:%S")
        );
    } else if parallel_jobs <= 1 {
        println!(
            "ℹ️  INFO [{}]: Running {} test(s) sequentially...",
            chrono::Local::now().format("%H:%M:%S"),
            test_files.len()
        );
    } else {
        println!(
            "ℹ️  INFO [{}]: Running {} test(s) in parallel (jobs: {})...",
            chrono::Local::now().format("%H:%M:%S"),
            test_files.len(),
            parallel_jobs
        );
    }

    // Setup Reporters
    let mut reporters: Vec<Box<dyn report::Reporter>> = Vec::new();

    // Create environment info
    let env_info = report::console::EnvironmentInfo {
        address: std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
            .unwrap_or_else(|_| config::default_address()),
        parallel_jobs,
        sort_mode: args.sort.clone(),
        dry_run: args.dry_run,
    };

    // Add streaming JSON reporter if --stream is enabled
    if args.stream {
        reporters.push(Box::new(report::StreamingJsonReporter::new(
            test_files.len(),
        )));
    } else {
        // Always add console reporter (unless streaming)
        let mode = match cli.progress_mode() {
            crate::cli::args::ProgressMode::Dots => report::ConsoleMode::Dots,
            crate::cli::args::ProgressMode::Verbose => report::ConsoleMode::Verbose,
            crate::cli::args::ProgressMode::None => report::ConsoleMode::Silent,
        };
        reporters.push(Box::new(report::ConsoleReporter::new(
            mode,
            test_files.len() as u64,
            env_info,
        )));
    }

    // Add file reporter if configured
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

    // Initialize state
    let mut test_results = TestResults::new();

    // Initialize Coverage Collector if requested
    let coverage_collector = if args.coverage {
        Some(Arc::new(report::CoverageCollector::new()))
    } else {
        None
    };

    // Execute tests
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

    // Move reporters to Arc
    let reporters: Arc<Vec<Box<dyn report::Reporter>>> = Arc::new(reporters);

    // Use a stream for bounded parallelism
    let stream = futures::stream::iter(test_files)
        .map(|file| {
            let runner = runner.clone();
            let reporters = reporters.clone();
            let file_path_str = file.to_string_lossy().to_string();
            let file_clone = file.clone();

            async move {
                // Notify start
                for r in reporters.iter() {
                    r.on_test_start(&file_path_str);
                }

                let test_start = std::time::Instant::now();
                let mut test_result = match run_single_test(
                    &runner,
                    &file_clone,
                    args.retry,
                    args.retry_delay,
                    args.no_retry,
                )
                .await
                {
                    Ok(res) => {
                        let call_duration = res.call_duration_ms;
                        let meta = res.meta;
                        match res.status {
                            execution::TestExecutionStatus::Pass => TestResult::pass_with_meta(
                                file_path_str.clone(),
                                0,
                                call_duration,
                                meta,
                            ),
                            execution::TestExecutionStatus::Fail(msg) => {
                                TestResult::fail_with_meta(
                                    file_path_str.clone(),
                                    msg,
                                    0,
                                    call_duration,
                                    meta,
                                )
                            }
                        }
                    }
                    Err(e) => TestResult::fail(
                        file_path_str.clone(),
                        format!("Execution error: {}", e),
                        0,
                        None,
                    ),
                };

                test_result.duration_ms = test_start.elapsed().as_millis() as u64;

                // Notify end
                for r in reporters.iter() {
                    r.on_test_end(&file_path_str, &test_result);
                }

                test_result
            }
        })
        .buffer_unordered(parallel_jobs);

    let results: Vec<TestResult> = stream.collect().await;

    // Collect results
    for result in results {
        test_results.add(result);
    }

    // Update metrics
    let total_duration = start_time.elapsed().as_millis() as u64;
    test_results.metrics.total_duration_ms = total_duration;
    test_results.metrics.parallel_jobs = parallel_jobs;

    // Notify suite end
    for r in reporters.iter() {
        r.on_suite_end(&test_results)?;
    }

    // Print Coverage Report if enabled
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

async fn run_single_test(
    runner: &execution::TestRunner,
    file: &std::path::Path,
    retry: u32,
    retry_delay: f64,
    no_retry: bool,
) -> Result<execution::TestExecutionResult> {
    // Parse document
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

    // Validate document
    if let Err(e) = parser::validate_document(&doc) {
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
        let current = runner.run_test(&doc).await?;

        let should_retry = match &current.status {
            execution::TestExecutionStatus::Pass => false,
            execution::TestExecutionStatus::Fail(msg) => should_retry_message(msg),
        };

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

    // Update file if requested
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
        assert!(!should_retry_message("gRPC error: code=5 message=not found"));
        assert!(!should_retry_message(
            "gRPC error: code=3 message=invalid argument"
        ));
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
}
