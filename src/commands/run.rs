// Run command - execute tests

use anyhow::Result;
use futures::stream::StreamExt;
use std::sync::Arc;
use tracing::{info, warn};

use crate::cli::Cli;
use crate::cli::args::RunArgs;
use crate::config;
use crate::execution;
use crate::parser;
use crate::report;
use crate::state::{TestResult, TestResults};
use crate::utils::FileUtils;

pub async fn run_tests(cli: &Cli, args: &RunArgs) -> Result<()> {
    // Get parallel job count
    let parallel_jobs = cli.parallel_jobs();
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
    for path in &args.test_paths {
        if path.is_dir() {
            test_files.extend(FileUtils::collect_test_files(path));
        } else if path.is_file() {
            test_files.push(path.clone());
        }
    }

    info!("Found {} test file(s)", test_files.len());

    if test_files.is_empty() {
        warn!("No test files found");
        return Ok(());
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
        reporters.push(Box::new(report::ConsoleReporter::new(
            cli.progress_mode(),
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
    let runner = Arc::new(execution::TestRunner::new(
        cli.run_args.dry_run,
        cli.run_args.timeout,
        cli.run_args.no_assert,
        cli.run_args.write,
        cli.verbose,
        coverage_collector.clone(),
    ));

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
                let mut test_result = match run_single_test(&runner, &file_clone).await {
                    Ok(res) => {
                        let grpc_duration = res.grpc_duration_ms;
                        match res.status {
                            execution::TestExecutionStatus::Pass => {
                                TestResult::pass(file_path_str.clone(), 0, grpc_duration)
                            }
                            execution::TestExecutionStatus::Fail(msg) => {
                                TestResult::fail(file_path_str.clone(), msg, 0, grpc_duration)
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
        if args.coverage_format == "json" {
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

    // Validate document
    if let Err(e) = parser::validate_document(&doc) {
        return Ok(execution::TestExecutionResult::fail(
            format!("Validation error: {}", e),
            None,
        ));
    }

    // Run test
    let result = runner.run_test(&doc).await?;

    // Update file if requested
    if let Some(resp) = &result.captured_response
        && let Err(e) = FileUtils::update_test_file(file, &doc, resp)
    {
        return Ok(execution::TestExecutionResult::fail(
            format!("Failed to update test file: {}", e),
            result.grpc_duration_ms,
        ));
    }

    Ok(result)
}
