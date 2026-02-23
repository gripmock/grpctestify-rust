// Main entry point for grpctestify

use anyhow::Result;
use clap::Parser;
use tracing::{error, info, warn};

// Import form library
use grpctestify::cli;
use grpctestify::config;
use grpctestify::execution;
// use grpctestify::grpc; // Unused
use grpctestify::parser;
use grpctestify::report;
use grpctestify::state;
use grpctestify::utils;

use cli::{
    args::{CheckArgs, ExplainArgs, FmtArgs, InspectArgs, ListArgs, LspArgs, ReflectArgs, RunArgs},
    Cli, Commands, LogFormat,
};
use grpctestify::grpc::client::{CompressionMode, GrpcClient, GrpcClientConfig, TlsConfig};
use report::{AllureReporter, ConsoleReporter, JsonReporter, JunitReporter, Reporter, StreamingJsonReporter};
use state::{TestResult, TestResults};
use utils::FileUtils;

use std::path::Path;
use std::sync::Arc;

use futures::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider (ring) to avoid panics with rustls 0.23+
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Load configuration from file (if exists)
    let config = config::Config::load();

    let cli = Cli::parse();

    // Setup tracing
    let filter = if cli.verbose {
        "grpctestify=debug,warn"
    } else {
        "grpctestify=warn,error"
    };

    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .event_format(grpctestify::logging::CustomFormatter)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .init();

    if cli.verbose {
        info!("Starting grpctestify v{}", env!("CARGO_PKG_VERSION"));
    }

    // Handle config flag
    if cli.config {
        println!("Current configuration:");
        println!("\n  Command-line arguments:");
        let args = cli.get_run_args();
        println!("    Parallel jobs: {}", args.parallel);
        println!("    Sort mode: {}", args.sort);
        println!("    Timeout: {}s", args.timeout);
        println!(
            "    Retry: {} times, {}s delay",
            args.retry, args.retry_delay
        );
        if let Some(ref log_format) = args.log_format {
            println!("    Log format: {}", log_format);
        }
        if let Some(ref log_output) = args.log_output {
            println!("    Log output: {}", log_output.display());
        }

        if let Some(cfg) = config {
            println!("\n  Configuration file loaded:");
            if !cfg.general.address.is_empty() {
                println!("    Address: {}", cfg.general.address);
            }
            if !cfg.general.parallel.is_empty() {
                println!("    Parallel: {}", cfg.general.parallel);
            }
            if cfg.general.timeout != 0 {
                println!("    Timeout: {}s", cfg.general.timeout);
            }
            if cfg.general.retry != 0 {
                println!("    Retry: {} times", cfg.general.retry);
            }
            if cfg.general.retry_delay != 0.0 {
                println!("    Retry delay: {}s", cfg.general.retry_delay);
            }
            if let Some(ref log_format) = cfg.general.log_format {
                println!("    Log format: {}", log_format);
            }
            if let Some(ref log_output) = cfg.general.log_output {
                println!("    Log output: {}", log_output);
            }
            println!("    Progress mode: {}", cfg.progress.mode);
            println!(
                "    Color: {}",
                if cfg.progress.color {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            if cfg.coverage.enabled {
                println!("    Coverage: enabled");
                if let Some(ref output) = cfg.coverage.output {
                    println!("      Output: {}", output);
                }
            }
        } else {
            println!("\n  No configuration file loaded");
            println!("  Create one with: grpctestify --init-config .grpctestifyrc.toml");
        }

        println!("\n  Environment variables:");
        if let Ok(addr) = std::env::var(config::ENV_GRPCTESTIFY_ADDRESS) {
            println!("    {}: {}", config::ENV_GRPCTESTIFY_ADDRESS, addr);
        } else {
            println!(
                "    {}: not set (default: {})",
                config::ENV_GRPCTESTIFY_ADDRESS,
                config::default_address()
            );
        }

        println!("\nConfiguration precedence:");
        println!("  1. Command-line arguments (highest)");
        println!("  2. Configuration file");
        println!("  3. Environment variables");
        println!("  4. Built-in defaults (lowest)");

        return Ok(());
    }

    // Handle init_config flag
    if let Some(config_file) = cli.init_config {
        let config = config::Config::default();
        let toml_content = config.to_toml();
        std::fs::write(&config_file, toml_content)?;
        println!("Configuration file created: {}", config_file.display());
        println!("\nYou can now edit the file to customize your settings.");
        println!("\nConfiguration precedence:");
        println!("  1. Command-line arguments (highest)");
        println!("  2. Configuration file");
        println!("  3. Environment variables");
        println!("  4. Built-in defaults (lowest)");
        return Ok(());
    }

    // Handle completion flag
    if let Some(shell_type) = cli.completion {
        handle_completion(&shell_type)?;
        return Ok(());
    }

    match &cli.command {
        Some(Commands::Reflect(args)) => handle_reflect(args).await,
        Some(Commands::Fmt(args)) => handle_fmt(args).await,
        Some(Commands::Check(args)) => handle_check(args).await,
        Some(Commands::Explain(args)) => handle_explain(args).await,
        Some(Commands::Inspect(args)) => handle_inspect(args).await,
        Some(Commands::List(args)) => handle_list(args),
        Some(Commands::Run(args)) => run_tests(&cli, args).await,
        Some(Commands::Lsp(args)) => handle_lsp(args).await,
        None => {
            // Implicit Run
            let args = cli.run_args.clone();
            if args.test_paths.is_empty() {
                // No paths provided. If dry-run is set, maybe okay?
                // But usually we expect paths.
                // Since `test_paths` is not required in Clap (to avoid conflict), we check here.
                warn!("No test files provided. Use 'grpctestify --help' for usage.");
                return Ok(());
            }
            run_tests(&cli, &args).await
        }
    }
}

fn handle_completion(shell_type: &str) -> Result<()> {
    use clap::CommandFactory;
    use clap_complete::{generate, Shell};

    let shell = match shell_type {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" => Shell::PowerShell,
        _ => {
            eprintln!("Error: Unsupported shell type '{}'", shell_type);
            eprintln!("Supported shells: bash, zsh, fish, elvish, powershell");
            return Err(anyhow::anyhow!("Unsupported shell type"));
        }
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell, &mut cmd, &bin_name, &mut std::io::stdout());

    Ok(())
}

async fn handle_reflect(args: &ReflectArgs) -> Result<()> {
    // Determine address
    let address = if let Some(addr) = &args.address {
        addr.clone()
    } else {
        std::env::var(config::ENV_GRPCTESTIFY_ADDRESS)
            .unwrap_or_else(|_| config::default_address())
    };

    // Default behavior: plaintext (no TLS)
    // --plaintext flag is kept for compatibility but has no effect (always plaintext by default)
    let use_plaintext = true; // Always use plaintext by default

    let tls_config = if use_plaintext {
        None  // plaintext (no TLS) - default
    } else {
        Some(TlsConfig {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: None,
            insecure_skip_verify: false,
        })
    };

    let config = GrpcClientConfig {
        address: address.clone(),
        timeout_seconds: 30,
        tls_config,
        proto_config: None,
        metadata: None,
        target_service: None,
        compression: CompressionMode::from_env(),
    };

    info!("Connecting to {} (plaintext: {})...", address, use_plaintext);
    
    let client = match GrpcClient::new(config).await {
        Ok(c) => c,
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("InvalidContentType") || err_msg.contains("corrupt message") {
                error!("Failed to connect: Server may not support gRPC Server Reflection Protocol.");
                error!("Try running gripmock with reflection enabled.");
                error!("You can test with: grpcurl -plaintext {} list", address);
            }
            return Err(e);
        }
    };

    let output = client.describe(args.symbol.as_deref())?;
    println!("{}", output);

    Ok(())
}

async fn handle_fmt(args: &FmtArgs) -> Result<()> {
    let mut files = Vec::new();
    let mut has_error = false;

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path));
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            error!("Path not found: {}", path.display());
            has_error = true;
        }
    }

    if files.is_empty() {
        if !has_error {
            warn!("No .gctf files found to format");
        }
        return Ok(());
    }

    for file in files {
        // Parse
        let doc = match parser::parse_gctf(&file) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        // Format/Serialize
        let formatted = serialize_gctf(&doc);

        if args.write {
            // Read original content to compare
            let original = std::fs::read_to_string(&file).unwrap_or_default();
            
            // Only write if content changed (idempotent check)
            if formatted != original {
                if let Err(e) = std::fs::write(&file, &formatted) {
                    error!("Failed to write {}: {}", file.display(), e);
                    has_error = true;
                }
                // Silent success - standard fmt behavior
            }
            // If content unchanged, no output (idempotent)
        } else {
            println!("{}", formatted);
        }
    }

    if has_error {
        std::process::exit(1);
    }

    Ok(())
}

fn handle_list(args: &ListArgs) -> Result<()> {
    let path = args.path.as_ref().map(|p| p.as_path()).unwrap_or_else(|| Path::new("."));

    if !path.exists() {
        error!("Path not found: {}", path.display());
        std::process::exit(1);
    }

    let files = FileUtils::collect_test_files(path);

    if args.format == "json" {
        let tests: Vec<serde_json::Value> = files
            .iter()
            .map(|file| {
                let relative = file.strip_prefix(path).unwrap_or(file);
                let id = relative.to_string_lossy().replace('\\', "/");
                let label = file.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| id.clone());
                let uri = format!("file://{}", file.canonicalize()
                    .unwrap_or_else(|_| file.to_path_buf())
                    .to_string_lossy()
                    .replace('\\', "/"));

                let mut test = serde_json::json!({
                    "id": id,
                    "label": label,
                    "uri": uri,
                    "children": []
                });

                if args.with_range {
                    if let Ok(doc) = parser::parse_gctf(file) {
                        let line_count = doc.metadata.source
                            .as_ref()
                            .map(|s| s.lines().count())
                            .unwrap_or(1);
                        test["range"] = serde_json::json!({
                            "start": {"line": 1, "column": 1},
                            "end": {"line": line_count, "column": 1}
                        });
                    }
                }

                test
            })
            .collect();

        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "tests": tests }))?);
    } else {
        for file in &files {
            println!("{}", file.display());
        }
    }

    Ok(())
}

async fn handle_lsp(_args: &LspArgs) -> Result<()> {
    use tower_lsp::{LspService, Server};
    
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    
    let (service, socket) = LspService::new(|client| {
        grpctestify::lsp::GrpctestifyLsp::new(client)
    });
    
    Server::new(stdin, stdout, socket).serve(service).await;
    
    Ok(())
}

// Re-export from lib
use grpctestify::serialize_gctf;

async fn handle_check(args: &CheckArgs) -> Result<()> {
    use report::{CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity};

    let mut files = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut files_with_errors = 0;

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path));
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            diagnostics.push(Diagnostic::error(
                &path.to_string_lossy(),
                "FILE_NOT_FOUND",
                "Path not found",
                1,
            ));
            files_with_errors += 1;
        }
    }

    if files.is_empty() {
    if args.format == "json" {
        let total_errors = diagnostics.iter().filter(|d| matches!(d.severity, DiagnosticSeverity::Error)).count();
        let total_warnings = diagnostics.iter().filter(|d| matches!(d.severity, DiagnosticSeverity::Warning)).count();
        let report = CheckReport {
            diagnostics,
            summary: CheckSummary {
                total_files: files.len(),
                files_with_errors,
                total_errors,
                total_warnings,
            },
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
        return Ok(());
    }

    info!("Checking {} file(s)...", files.len());

    for file in &files {
        let file_str = file.to_string_lossy().to_string();
        match parser::parse_gctf(file) {
            Ok(doc) => {
                // Check for deprecated HEADERS using AST section types
                for section in &doc.sections {
                    // Parser normalizes HEADERS to REQUEST_HEADERS, but we can check raw content
                    if let Some(source) = &doc.metadata.source {
                        let lines: Vec<&str> = source.lines().collect();
                        if section.start_line < lines.len() {
                            let line = lines[section.start_line].trim();
                            if line.to_uppercase() == "--- HEADERS ---" {
                                diagnostics.push(Diagnostic::warning(
                                    &file_str,
                                    "DEPRECATED_SECTION",
                                    "HEADERS section is deprecated, use REQUEST_HEADERS instead",
                                    section.start_line + 1,
                                ).with_hint("Replace --- HEADERS --- with --- REQUEST_HEADERS ---"));
                            }
                        }
                    }
                }

                if let Err(e) = parser::validate_document(&doc) {
                    diagnostics.push(Diagnostic::error(
                        &file_str,
                        "VALIDATION_ERROR",
                        &e.to_string(),
                        1,
                    ));
                    files_with_errors += 1;
                } else if args.format != "json" {
                    println!("{} ... OK", file.display());
                }
            }
            Err(e) => {
                diagnostics.push(Diagnostic::error(
                    &file_str,
                    "PARSE_ERROR",
                    &e.to_string(),
                    1,
                ));
                files_with_errors += 1;
            }
        }
    }

    if args.format == "json" {
        let total_errors = diagnostics.iter().filter(|d| matches!(d.severity, DiagnosticSeverity::Error)).count();
        let total_warnings = diagnostics.iter().filter(|d| matches!(d.severity, DiagnosticSeverity::Warning)).count();
        let report = CheckReport {
            diagnostics,
            summary: CheckSummary {
                total_files: files.len(),
                files_with_errors,
                total_errors,
                total_warnings,
            },
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    if files_with_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

async fn handle_explain(args: &ExplainArgs) -> Result<()> {
    let file_path = &args.file;
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_start = std::time::Instant::now();
    let (doc, _) = parser::parse_gctf_with_diagnostics(file_path)?;
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    let validation_start = std::time::Instant::now();
    let validation_result = parser::validate_document(&doc);
    let validation_ms = validation_start.elapsed().as_secs_f64() * 1000.0;

    if args.format == "json" {
        // Build execution plan and output as JSON
        let execution_plan = execution::ExecutionPlan::from_document(&doc);
        println!("{}", serde_json::to_string_pretty(&execution_plan)?);
    } else {
        // Print workflow visualization
        let execution_plan = execution::ExecutionPlan::from_document(&doc);
        print_workflow_from_plan(&execution_plan);
        println!();

        // Show validation result
        println!("Validation:");
        match validation_result {
            Ok(_) => println!("  ✅ No issues found. Test appears structurally valid."),
            Err(e) => println!("  ❌ Validation failed: {}", e),
        }
        println!();
        println!("Parse time: {:.3}ms, Validation time: {:.3}ms", parse_ms, validation_ms);
    }

    Ok(())
}

async fn handle_inspect(args: &InspectArgs) -> Result<()> {
    use report::{AstOverview, InspectReport, SectionInfo};

    let file_path = &args.file;
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_start = std::time::Instant::now();
    let (doc, parse_diagnostics) = parser::parse_gctf_with_diagnostics(file_path)?;
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    let validation_start = std::time::Instant::now();
    let validation_result = parser::validate_document(&doc);
    let validation_ms = validation_start.elapsed().as_secs_f64() * 1000.0;

    if args.format == "json" {
        let mut diagnostics: Vec<report::Diagnostic> = Vec::new();
        let file_str = file_path.to_string_lossy().to_string();

        if let Err(e) = &validation_result {
            diagnostics.push(report::Diagnostic::error(
                &file_str,
                "VALIDATION_ERROR",
                &e.to_string(),
                1,
            ));
        }

        // Check for deprecated HEADERS using AST
        for section in &doc.sections {
            if let Some(source) = &doc.metadata.source {
                let lines: Vec<&str> = source.lines().collect();
                if section.start_line < lines.len() {
                    let line = lines[section.start_line].trim();
                    if line.to_uppercase() == "--- HEADERS ---" {
                        diagnostics.push(report::Diagnostic::warning(
                            &file_str,
                            "DEPRECATED_SECTION",
                            "HEADERS section is deprecated, use REQUEST_HEADERS instead",
                            section.start_line + 1,
                        ).with_hint("Replace --- HEADERS --- with --- REQUEST_HEADERS ---"));
                    }
                }
            }
        }

        // Semantic checks - Inspect Intelligence Layer
        let sections = &doc.sections;
        for (i, section) in sections.iter().enumerate() {
            // Check for `with_asserts` without following ASSERTS
            if section.inline_options.with_asserts {
                let has_following_asserts = sections[i + 1..]
                    .iter()
                    .take_while(|s| s.section_type != parser::ast::SectionType::Request)
                    .any(|s| s.section_type == parser::ast::SectionType::Asserts);
                if !has_following_asserts {
                    diagnostics.push(report::Diagnostic::warning(
                        &file_str,
                        "ORPHAN_WITH_ASSERTS",
                        "with_asserts option set but no ASSERTS section follows",
                        section.start_line,
                    ).with_hint("Add ASSERTS section after this response or remove with_asserts"));
                }
            }

            // Check for ASSERTS without preceding response with_asserts
            if section.section_type == parser::ast::SectionType::Asserts {
                let has_preceding_with_asserts = sections[..i]
                    .iter()
                    .rev()
                    .take_while(|s| s.section_type != parser::ast::SectionType::Request)
                    .any(|s| s.inline_options.with_asserts);
                if !has_preceding_with_asserts {
                    diagnostics.push(report::Diagnostic::info(
                        &file_str,
                        "ASSERTS_WITHOUT_WITH_ASSERTS",
                        "ASSERTS section without with_asserts on preceding response",
                        section.start_line,
                    ).with_hint("Consider adding with_asserts option to the RESPONSE section"));
                }
            }

            // Streaming flow risk: multiple RESPONSE sections
            if section.section_type == parser::ast::SectionType::Response {
                let response_count = sections.iter().filter(|s| s.section_type == parser::ast::SectionType::Response).count();
                if response_count > 1 && !section.inline_options.unordered_arrays {
                    diagnostics.push(report::Diagnostic::hint(
                        &file_str,
                        "STREAMING_ORDER_HINT",
                        "Multiple responses in streaming test - order matters by default",
                        section.start_line,
                    ).with_hint("Use unordered_arrays option if order doesn't matter"));
                }
            }

            // Check for empty REQUEST sections (valid but notable)
            if section.section_type == parser::ast::SectionType::Request {
                if matches!(section.content, parser::ast::SectionContent::Empty) {
                    diagnostics.push(report::Diagnostic::info(
                        &file_str,
                        "EMPTY_REQUEST",
                        "Empty REQUEST section will send empty JSON object",
                        section.start_line,
                    ).with_hint("This is valid - sends {}"));
                }
            }
        }

        // Check for missing sections
        let has_endpoint = sections.iter().any(|s| s.section_type == parser::ast::SectionType::Endpoint);
        if !has_endpoint {
            diagnostics.push(report::Diagnostic::error(
                &file_str,
                "MISSING_ENDPOINT",
                "No ENDPOINT section found",
                1,
            ).with_hint("Add --- ENDPOINT --- section with Service/Method"));
        }

        let has_request_or_response = sections.iter().any(|s|
            s.section_type == parser::ast::SectionType::Request ||
            s.section_type == parser::ast::SectionType::Response
        );
        if !has_request_or_response {
            diagnostics.push(report::Diagnostic::warning(
                &file_str,
                "NO_REQUEST_RESPONSE",
                "No REQUEST or RESPONSE sections found",
                1,
            ).with_hint("Add REQUEST and/or RESPONSE sections"));
        }

        // Build sections info
        let sections_info: Vec<SectionInfo> = doc.sections.iter().map(|s| {
            let content_kind = match &s.content {
                parser::ast::SectionContent::Single(_) => "single",
                parser::ast::SectionContent::Json(_) => "json",
                parser::ast::SectionContent::JsonLines(_) => "json-lines",
                parser::ast::SectionContent::KeyValues(_) => "key-values",
                parser::ast::SectionContent::Assertions(_) => "assertions",
                parser::ast::SectionContent::Extract(_) => "extract",
                parser::ast::SectionContent::Empty => "empty",
            };
            let message_count = match &s.content {
                parser::ast::SectionContent::JsonLines(lines) => Some(lines.len()),
                _ => None,
            };
            SectionInfo {
                section_type: s.section_type.as_str().to_string(),
                start_line: s.start_line,
                end_line: s.end_line,
                content_kind: content_kind.to_string(),
                message_count,
            }
        }).collect();

        // Infer RPC mode from execution plan
        let execution_plan = execution::ExecutionPlan::from_document(&doc);
        let inferred_rpc_mode = Some(execution_plan.summary.rpc_mode_name);

        let report = InspectReport {
            file: file_str,
            parse_time_ms: parse_ms,
            validation_time_ms: validation_ms,
            ast: AstOverview { sections: sections_info },
            diagnostics,
            inferred_rpc_mode,
        };

        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        // Print technical AST analysis (without workflow)
        print_analysis(
            &doc,
            file_path,
            &parse_diagnostics,
            validation_ms,
            validation_result.err().map(|e| e.to_string()).as_deref(),
        );
    }

    Ok(())
}

/// Print workflow visualization from ExecutionPlan
fn print_workflow_from_plan(plan: &execution::ExecutionPlan) {
    use execution::RpcMode;
    
    println!();
    println!("[Execution Workflow]");
    println!("Workflow for {}:", plan.file_path);
    println!("{:-<60}", "");

    // RPC Mode
    let rpc_desc = match &plan.rpc_mode {
        RpcMode::Unary => "Standard request-response pattern".to_string(),
        RpcMode::UnaryError => "Single request expecting gRPC error response".to_string(),
        RpcMode::ServerStreaming { response_count } => 
            format!("Single request, {} responses - server sends stream", response_count),
        RpcMode::ClientStreaming { request_count } => 
            format!("{} requests sent, single response expected", request_count),
        RpcMode::BidirectionalStreaming { request_count, response_count } => 
            format!("{} requests and {} responses - full duplex stream", request_count, response_count),
        RpcMode::Unknown => "Unusual pattern".to_string(),
    };
    println!("RPC Mode: {}", plan.summary.rpc_mode_name);
    println!("  {}", rpc_desc);
    println!();

    // Phase 1: Connection
    println!("Phase 1: Connection");
    println!("  Connect to gRPC server: {}", plan.connection.address);
    println!("  Source: {}", plan.connection.source);
    println!();

    // Phase 2: Target
    println!("Phase 2: Target Endpoint");
    println!("  gRPC Method: {}", plan.target.endpoint);
    if let Some(pkg) = &plan.target.package {
        println!("  - Package:  {}", pkg);
    }
    if let Some(svc) = &plan.target.service {
        println!("  - Service:  {}", svc);
    }
    if let Some(method) = &plan.target.method {
        println!("  - Method:   {}", method);
    }
    println!();

    // Phase 3: Headers
    if let Some(headers) = &plan.headers {
        println!("Phase 3: Request Headers");
        println!("  Headers: {} key(s)", headers.count);
        for (k, v) in &headers.headers {
            println!("  - {}: {}", k, truncate_str(v, 50));
        }
        println!();
    }

    // Phase 4: Request Flow
    println!("Phase 4: Request Flow");
    if plan.requests.is_empty() {
        println!("  No REQUEST sections - will send empty JSON object {{}}");
        println!("  (implicit behavior for server-streaming calls)");
    } else {
        for req in &plan.requests {
            println!("  Request #{} [Line {}-{}]", req.index, req.line_start, req.line_end);
            let json_str = serde_json::to_string_pretty(&req.content).unwrap_or_default();
            let lines: Vec<&str> = json_str.lines().collect();
            if lines.len() <= 5 {
                for line in lines {
                    println!("    {}", line);
                }
            } else {
                for line in lines.iter().take(5) {
                    println!("    {}", line);
                }
                println!("    ... ({} more lines)", lines.len() - 5);
            }
        }
    }
    println!();

    // Phase 5: Expected Response/Error
    println!("Phase 5: Expected Response / Error");
    if plan.expectations.is_empty() {
        println!("  No RESPONSE or ERROR section - implicit success expected");
    } else {
        for exp in &plan.expectations {
            if exp.expectation_type == "error" {
                println!("  Expected Error [Line {}-{}]", exp.line_start, exp.line_end);
                if let Some(content) = &exp.content {
                    let json_str = serde_json::to_string_pretty(content).unwrap_or_default();
                    let lines: Vec<&str> = json_str.lines().collect();
                    for line in lines.iter().take(6) {
                        println!("    {}", line);
                    }
                    if lines.len() > 6 {
                        println!("    ... ({} more lines)", lines.len() - 6);
                    }
                }
                println!("  Will verify gRPC error status & message");
            } else {
                println!("  Expected Response #{} [Line {}-{}]", exp.index, exp.line_start, exp.line_end);
                if let Some(content) = &exp.content {
                    let json_str = serde_json::to_string_pretty(content).unwrap_or_default();
                    let lines: Vec<&str> = json_str.lines().collect();
                    if lines.len() <= 5 {
                        for line in lines {
                            println!("    {}", line);
                        }
                    } else {
                        for line in lines.iter().take(5) {
                            println!("    {}", line);
                        }
                        println!("    ... ({} more lines)", lines.len() - 5);
                    }
                } else if let Some(count) = exp.message_count {
                    println!("    Expecting {} messages (streaming)", count);
                }

                // Show comparison options
                let opts = &exp.comparison_options;
                if opts.partial || !opts.redact.is_empty() || opts.tolerance.is_some() || opts.unordered_arrays || opts.with_asserts {
                    println!("  Comparison Options:");
                    if opts.partial { println!("    - Partial Match: enabled"); }
                    if !opts.redact.is_empty() { println!("    - Redact Fields: {:?}", opts.redact); }
                    if let Some(tol) = opts.tolerance { println!("    - Tolerance: {}", tol); }
                    if opts.unordered_arrays { println!("    - Unordered Arrays: enabled"); }
                    if opts.with_asserts { println!("    - Run Asserts: enabled"); }
                }
            }
        }
    }
    println!();

    // Phase 6: Assertions
    if !plan.assertions.is_empty() {
        println!("Phase 6: Assertions");
        for assertion in &plan.assertions {
            println!("  Assertion Block #{} [Line {}-{}]", assertion.index, assertion.line_start, assertion.line_end);
            for assert in &assertion.assertions {
                println!("  - {}", assert);
            }
        }
        println!();
    }

    // Phase 7: Variable Extraction
    if !plan.extractions.is_empty() {
        println!("Phase 7: Variable Extraction");
        for ext in &plan.extractions {
            println!("  Extract [Line {}-{}]", ext.line_start, ext.line_end);
            for (k, v) in &ext.variables {
                println!("  - ${} = {}", k, v);
            }
        }
        println!();
    }

    // Execution Summary
    println!("Execution Summary:");
    println!("  Total Requests:       {}", plan.summary.total_requests);
    println!("  Total Responses:      {}", plan.summary.total_responses);
    println!("  Error Expected:       {}", if plan.summary.error_expected { "Yes" } else { "No" });
    println!("  Assertion Blocks:     {}", plan.summary.assertion_blocks);
    println!("  Variable Extractions: {}", plan.summary.variable_extractions);
    println!("  RPC Mode:             {}", plan.summary.rpc_mode_name);
    println!();
}

/// Truncate string to max length with ellipsis
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn print_analysis(
    doc: &parser::GctfDocument,
    file_path: &std::path::Path,
    parse_diagnostics: &parser::ParseDiagnostics,
    validation_ms: f64,
    validation_error: Option<&str>,
) {
    println!("Analysis Report for {}:", file_path.display());
    println!("{:=<60}", "");

    println!("\n[Parse Profiling]");
    println!("  File size: {} bytes", parse_diagnostics.bytes);
    println!("  Total lines: {}", parse_diagnostics.total_lines);
    println!("  Section headers found: {}", parse_diagnostics.section_headers);
    println!(
        "  Parse total: {:.3}ms (read: {:.3}ms, parse-sections: {:.3}ms, build-doc: {:.3}ms)",
        parse_diagnostics.timings.total_ms,
        parse_diagnostics.timings.read_ms,
        parse_diagnostics.timings.parse_sections_ms,
        parse_diagnostics.timings.build_document_ms,
    );
    println!("  Validation: {:.3}ms", validation_ms);
    if let Some(err) = validation_error {
        println!("  Validation result: FAILED ({})", err);
    } else {
        println!("  Validation result: OK");
    }

    println!("\n[AST Overview]");
    print_ast_overview(doc);

    // 1. Structure Check
    println!("\n[Structure]");
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    if doc.get_endpoint().is_none() {
        errors.push("Missing ENDPOINT section. Test cannot run.".to_string());
    }

    if doc.has_response_error_conflict() {
        errors.push("Conflict: Both RESPONSE and ERROR sections present. Usually mutually exclusive.".to_string());
    }

    let request_count = doc.sections_by_type(parser::ast::SectionType::Request).len();
    if request_count == 0 {
        warnings.push("No REQUEST sections found. Assuming empty request or implicit defaults.".to_string());
    }

    // 2. Variable Usage (Static Analysis)
    println!("\n[Variables]");
    let mut defined_vars = std::collections::HashSet::new();
    let mut used_vars = std::collections::HashSet::new();

    // Find definitions (EXTRACT)
    for section in doc.sections_by_type(parser::ast::SectionType::Extract) {
        if let parser::ast::SectionContent::Extract(vars) = &section.content {
            for k in vars.keys() {
                defined_vars.insert(k.clone());
            }
        }
    }

    // Find usages (${VAR}) in all relevant sections
    // This is a naive regex scan over raw content for simplicity
    let var_regex = regex::Regex::new(r"\$\{([a-zA-Z0-9_]+)\}").unwrap();
    
    for section in &doc.sections {
        for cap in var_regex.captures_iter(&section.raw_content) {
            if let Some(m) = cap.get(1) {
                used_vars.insert(m.as_str().to_string());
            }
        }
    }

    if defined_vars.is_empty() && used_vars.is_empty() {
        println!("  No variables defined or used.");
    } else {
        println!("  Defined: {:?}", defined_vars);
        println!("  Used:    {:?}", used_vars);

        // Check for undefined variables
        for usage in &used_vars {
            if !defined_vars.contains(usage) {
                 // Check if it might be an environment variable (by convention often UPPERCASE)
                 if usage.to_uppercase() == *usage {
                     println!("  - Note: ${} likely expects an environment variable.", usage);
                 } else {
                     warnings.push(format!("Variable '${}' is used but not defined in EXTRACT sections.", usage));
                 }
            }
        }

        // Check for unused variables
        for def in &defined_vars {
            if !used_vars.contains(def) {
                warnings.push(format!("Variable '{}' is extracted but never used in this file.", def));
            }
        }
    }

    // 3. Logic/Flow
    println!("\n[Logic Flow]");
    let is_streaming = request_count > 1;
    let response_count = doc.sections_by_type(parser::ast::SectionType::Response).len();
    
    if is_streaming {
        println!("  - Detected Client Streaming ({} requests)", request_count);
        if response_count > 1 {
             println!("  - Detected Server Streaming/Bidi ({} responses)", response_count);
        } else {
             println!("  - Detected Client Streaming -> Unary Response");
        }
    } else {
        if response_count > 1 {
             println!("  - Detected Server Streaming (1 request -> {} responses)", response_count);
        } else {
             println!("  - Standard Unary Call");
        }
    }

    // 4. Summary
    println!("\n[Summary]");
    if errors.is_empty() && warnings.is_empty() {
        println!("  ✅ No issues found. Test appears structurally valid.");
    } else {
        if !errors.is_empty() {
            println!("  ❌ Errors (must fix):");
            for e in &errors {
                println!("     - {}", e);
            }
        }
        if !warnings.is_empty() {
            println!("  ⚠️  Warnings (check logic):");
            for w in &warnings {
                println!("     - {}", w);
            }
        }
    }
}

fn print_ast_overview(doc: &parser::GctfDocument) {
    if doc.sections.is_empty() {
        println!("  No sections found.");
        return;
    }

    for (idx, section) in doc.sections.iter().enumerate() {
        let content_kind = match section.content {
            parser::ast::SectionContent::Single(_) => "single",
            parser::ast::SectionContent::Json(_) => "json",
            parser::ast::SectionContent::JsonLines(_) => "json-lines",
            parser::ast::SectionContent::KeyValues(_) => "key-values",
            parser::ast::SectionContent::Extract(_) => "extract",
            parser::ast::SectionContent::Assertions(_) => "assertions",
            parser::ast::SectionContent::Empty => "empty",
        };

        let content_size = section.raw_content.len();
        println!(
            "  {:>2}. {:<16} lines {:>3}-{:>3} | {:<10} | raw={} bytes",
            idx + 1,
            section.section_type.as_str(),
            section.start_line,
            section.end_line,
            content_kind,
            content_size
        );

        if section.inline_options.partial
            || section.inline_options.with_asserts
            || section.inline_options.unordered_arrays
            || section.inline_options.tolerance.is_some()
            || !section.inline_options.redact.is_empty()
        {
            println!(
                "      options: partial={}, with_asserts={}, unordered_arrays={}, tolerance={:?}, redact={:?}",
                section.inline_options.partial,
                section.inline_options.with_asserts,
                section.inline_options.unordered_arrays,
                section.inline_options.tolerance,
                section.inline_options.redact
            );
        }
    }
}

async fn run_tests(cli: &Cli, args: &RunArgs) -> Result<()> {
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
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();

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
        reporters.push(Box::new(StreamingJsonReporter::new(test_files.len())));
    } else {
        // Always add console reporter (unless streaming)
        reporters.push(Box::new(ConsoleReporter::new(
            cli.progress_mode(),
            test_files.len() as u64,
            env_info,
        )));
    }

    // Add file reporter if configured
    if let Some(format) = cli.log_format_mode() {
        if let Some(output_path) = &args.log_output {
            match format {
                LogFormat::Json => {
                    reporters.push(Box::new(JsonReporter::new(output_path.clone())));
                }
                LogFormat::JUnit => {
                    reporters.push(Box::new(JunitReporter::new(output_path.clone())));
                }
                LogFormat::Allure => {
                    reporters.push(Box::new(AllureReporter::new(output_path.clone())));
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
    // Pass args to TestRunner if needed, e.g. no_assert
    // We need to update TestRunner to accept no_assert
    let runner = Arc::new(execution::TestRunner::new(
        cli.run_args.dry_run,
        cli.run_args.timeout,
        cli.run_args.no_assert,
        cli.run_args.update,
        cli.verbose,
        coverage_collector.clone(),
    ));

    // Move reporters to Arc
    let reporters: Arc<Vec<Box<dyn Reporter>>> = Arc::new(reporters);

    // Use a stream for bounded parallelism (Work Stealing pattern)
    // This is more efficient than spawning all tasks at once with a semaphore,
    // as it keeps only N futures active at a time.
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
    let _file_path = file.to_string_lossy();

    // Parse document
    let doc = match parser::parse_gctf(file) {
        Ok(d) => d,
        Err(e) => {
            return Ok(execution::TestExecutionResult::fail(
                format!("Parse error: {}", e),
                None,
            ))
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
    if let Some(resp) = &result.captured_response {
        // Assuming runner.update_mode is true if captured_response is present
        if let Err(e) = utils::FileUtils::update_test_file(file, &doc, resp) {
            return Ok(execution::TestExecutionResult::fail(
                format!("Failed to update test file: {}", e),
                None,
            ));
        }
        // If update successful, force pass?
        // result.status = execution::TestExecutionStatus::Pass;
    }

    Ok(result)
}

