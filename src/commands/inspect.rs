// Inspect command - show detailed AST analysis and structure

use crate::cli::args::HasFormat;
use crate::utils::file::FileUtils;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::bench::schema::bench_key_rank;
use crate::cli::args::InspectArgs;
use crate::commands::bench::BenchConfigResolved;
use crate::execution::runner_helpers::{
    CliRuntimeDefaults, EffectiveRuntimeOptions, resolve_effective_runtime_options,
};
use crate::execution::{ExecutionPlan, Workflow};
use crate::parser;
use crate::parser::ast::{SectionContent, SectionType};
use crate::report::{AstOverview, InspectReport, SectionInfo};

fn bench_resolved_options(
    doc: &parser::GctfDocument,
) -> Option<Vec<crate::report::BenchResolvedOption>> {
    let bench_section = bench_section_map(doc)?;
    let config = BenchConfigResolved::from_bench_section(Some(bench_section)).ok()?;

    let mut out = Vec::new();
    for (internal_key, output_key) in [
        ("concurrency", "concurrency"),
        ("load_schedule", "load_schedule"),
        ("load_start", "load_start"),
        ("load_step", "load_step"),
        ("load_end", "load_end"),
        ("load_step_duration", "load_step_duration"),
        ("load_max_duration", "load_max_duration"),
        ("progress_interval", "progress_interval"),
    ] {
        let source = config
            .option_sources
            .get(internal_key)
            .copied()
            .unwrap_or(crate::commands::bench::BenchOptionSource::Default)
            .as_str()
            .to_string();
        let value = match internal_key {
            "concurrency" => config.concurrency.to_string(),
            "load_schedule" => config.load_schedule.clone(),
            "load_start" => config
                .load_start
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            "load_step" => config
                .load_step
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            "load_end" => config
                .load_end
                .map(|v| v.to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            "load_step_duration" => config
                .load_step_duration
                .map(|v| format!("{}ms", v.as_millis()))
                .unwrap_or_else(|| "<none>".to_string()),
            "load_max_duration" => config
                .load_max_duration
                .map(|v| format!("{}ms", v.as_millis()))
                .unwrap_or_else(|| "<none>".to_string()),
            "progress_interval" => format!("{}ms", config.progress_interval.as_millis()),
            _ => "<n/a>".to_string(),
        };

        out.push(crate::report::BenchResolvedOption {
            key: output_key.to_string(),
            value,
            source,
        });
    }

    Some(out)
}

fn workflow_validation_errors(workflow: &Workflow) -> Vec<String> {
    for event in workflow.validation_results() {
        if let crate::execution::WorkflowEvent::ValidationResult { passed, errors } = event
            && !passed
        {
            return errors.clone();
        }
    }
    Vec::new()
}

fn workflow_optimizer_hints(
    workflow: &Workflow,
) -> Vec<crate::execution::workflow_events::OptimizationHint> {
    for event in workflow.optimization_hints() {
        if let crate::execution::WorkflowEvent::OptimizationFound { hints } = event {
            return hints.clone();
        }
    }
    Vec::new()
}

fn sort_report_diagnostics(diags: &mut [crate::report::Diagnostic]) {
    diags.sort_by(|a, b| {
        (a.range.start.line, a.code.as_str(), a.message.as_str()).cmp(&(
            b.range.start.line,
            b.code.as_str(),
            b.message.as_str(),
        ))
    });
}

pub async fn handle_inspect(args: &InspectArgs) -> Result<()> {
    let file_path = &args.file;
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_start = std::time::Instant::now();
    let parse_result = parser::parse_with_recovery(file_path);
    let doc = parse_result.document;
    let diagnostics = parse_result.diagnostics;
    let parse_diagnostics = parser::parse_gctf_with_diagnostics(file_path)
        .ok()
        .map(|(_, d)| d)
        .unwrap_or_default();
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    if !diagnostics.is_empty() {
        eprintln!();
        eprintln!("PARSE DIAGNOSTICS");
        eprintln!("=================");
        eprintln!("File: {}", file_path.display());
        eprintln!("Recovered sections: {}", parse_result.recovered_sections);
        eprintln!("Failed sections: {}", parse_result.failed_sections);
        eprintln!();
        for diagnostic in &diagnostics.diagnostics {
            crate::commands::print_diagnostic(diagnostic);
            eprintln!();
        }
    }

    let validation_start = std::time::Instant::now();
    let workflow = Workflow::from_document_with_analysis(&doc);
    let validation_errors = workflow_validation_errors(&workflow);
    let validation_ms = validation_start.elapsed().as_secs_f64() * 1000.0;

    if args.is_json() {
        print_json_report(&doc, file_path, parse_ms, validation_ms);
    } else {
        print_detailed_analysis(
            &doc,
            &workflow,
            file_path,
            &parse_diagnostics,
            parse_ms,
            validation_ms,
            validation_errors.first().map(String::as_str),
        );
    }

    Ok(())
}

fn print_json_report(
    doc: &parser::GctfDocument,
    file_path: &Path,
    parse_ms: f64,
    validation_ms: f64,
) {
    let mut inspect_diagnostics: Vec<crate::report::Diagnostic> = Vec::new();
    let mut semantic_diagnostics: Vec<crate::report::Diagnostic> = Vec::new();
    let mut optimization_hints: Vec<crate::report::Diagnostic> = Vec::new();
    let file_str = file_path.to_string_lossy().to_string();

    for (doc_idx, d) in doc.iter_chain().enumerate() {
        let w = Workflow::from_document_with_analysis(d);
        for err in workflow_validation_errors(&w) {
            let msg = if doc.is_single_document() {
                err
            } else {
                format!("Document {}: {}", doc_idx + 1, err)
            };
            inspect_diagnostics.push(crate::report::Diagnostic::error(
                &file_str,
                "VALIDATION_ERROR",
                &msg,
                1,
            ));
        }
        for section in &d.sections {
            if doc.section_uses_deprecated_headers_alias(section) {
                inspect_diagnostics.push(
                    crate::report::Diagnostic::warning(
                        &file_str,
                        "DEPRECATED_SECTION",
                        "HEADERS section is deprecated, use REQUEST_HEADERS instead",
                        section.start_line + 1,
                    )
                    .with_hint("Replace --- HEADERS --- with --- REQUEST_HEADERS ---"),
                );
            }

            if !section.attributes.is_empty() {
                for attr in &section.attributes {
                    if attr.name == "skip" && attr.value == "true" {
                        inspect_diagnostics.push(
                            crate::report::Diagnostic::info(
                                &file_str,
                                "ATTRIBUTE_SKIP",
                                &format!(
                                    "{} section at line {} is skipped via #[skip]",
                                    section.section_type.as_str(),
                                    section.start_line + 1
                                ),
                                section.start_line + 1,
                            )
                            .with_hint("Remove #[skip] attribute to enable this section"),
                        );
                    } else if attr.name == "timeout"
                        && let Ok(secs) = attr.value.parse::<u64>()
                        && secs == 0
                    {
                        inspect_diagnostics.push(crate::report::Diagnostic::warning(
                            &file_str,
                            "ATTRIBUTE_TIMEOUT_ZERO",
                            &format!(
                                "{} section at line {} has timeout=0 (no timeout)",
                                section.section_type.as_str(),
                                section.start_line + 1
                            ),
                            section.start_line + 1,
                        ));
                    }
                }
            }
        }
        for event in w.semantic_analysis() {
            if let crate::execution::WorkflowEvent::SemanticAnalysis {
                type_mismatches,
                unknown_plugins,
            } = event
            {
                for mismatch in type_mismatches {
                    semantic_diagnostics.push(
                        crate::report::Diagnostic::error(
                            &file_str,
                            &mismatch.rule_id,
                            &mismatch.message,
                            mismatch.line,
                        )
                        .with_hint(
                            &mismatch
                                .expression
                                .as_ref()
                                .map(|e| format!("Expression: {}", e))
                                .unwrap_or_default(),
                        ),
                    );
                }
                for unknown in unknown_plugins {
                    semantic_diagnostics.push(
                        crate::report::Diagnostic::error(
                            &file_str,
                            &unknown.rule_id,
                            &unknown.message,
                            unknown.line,
                        )
                        .with_hint(
                            &unknown
                                .expression
                                .as_ref()
                                .map(|e| format!("Assertion: {}", e))
                                .unwrap_or_default(),
                        ),
                    );
                }
            }
        }
        for hint in workflow_optimizer_hints(&w) {
            optimization_hints.push(
                crate::report::Diagnostic::hint(
                    &file_str,
                    &hint.rule_id,
                    &format!("Safe rewrite available: {} -> {}", hint.before, hint.after),
                    hint.line,
                )
                .with_hint("Boolean expression compared with true/false can be simplified"),
            );
        }
    }

    sort_report_diagnostics(&mut inspect_diagnostics);
    sort_report_diagnostics(&mut semantic_diagnostics);
    sort_report_diagnostics(&mut optimization_hints);

    let mut sections_info: Vec<SectionInfo> = Vec::new();
    for d in doc.iter_chain() {
        for s in &d.sections {
            sections_info.push(section_to_info(s));
        }
    }

    let plan = ExecutionPlan::from_document(doc);
    let effective_runtime = resolve_effective_runtime_options(
        doc,
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    )
    .ok();
    let report = InspectReport {
        file: file_str,
        parse_time_ms: parse_ms,
        validation_time_ms: validation_ms,
        ast: AstOverview {
            sections: sections_info,
        },
        diagnostics: inspect_diagnostics,
        semantic_diagnostics,
        optimization_hints,
        inferred_rpc_mode: Some(plan.summary.rpc_mode_name.clone()),
        effective_runtime: effective_runtime.map(|r| serde_json::to_value(&r).unwrap_or_default()),
        bench_resolved: bench_resolved_options(doc),
    };
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Failed to serialize inspect report: {}", e),
    }
}

fn section_to_info(section: &parser::ast::Section) -> SectionInfo {
    let content_kind = match &section.content {
        SectionContent::Single(_) => "single",
        SectionContent::Json(_) => "json",
        SectionContent::JsonLines(_) => "json-lines",
        SectionContent::KeyValues(_) => "key-values",
        SectionContent::Assertions(_) => "assertions",
        SectionContent::Extract(_) => "extract",
        SectionContent::Meta(_) => "meta",
        SectionContent::Empty => "empty",
    };
    let message_count = match &section.content {
        SectionContent::JsonLines(lines) => Some(lines.len()),
        _ => None,
    };
    SectionInfo {
        section_type: section.section_type.as_str().to_string(),
        start_line: section.start_line,
        end_line: section.end_line,
        content_kind: content_kind.to_string(),
        message_count,
    }
}

fn print_detailed_analysis(
    doc: &parser::GctfDocument,
    workflow: &Workflow,
    file_path: &Path,
    parse_diagnostics: &parser::ParseDiagnostics,
    parse_ms: f64,
    validation_ms: f64,
    validation_error: Option<&str>,
) {
    let effective_runtime = resolve_effective_runtime_options(
        doc,
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    )
    .ok();

    println!();
    println!("ANALYSIS REPORT");
    println!("===============");
    println!();
    println!("FILE: {}", file_path.display());
    println!();

    println!("PARSE PROFILING");
    println!("---------------");
    println!("  File size:         {} bytes", parse_diagnostics.bytes);
    println!("  Total lines:       {}", parse_diagnostics.total_lines);
    println!("  Section headers:   {}", parse_diagnostics.section_headers);
    println!("  Parse total:       {:.3}ms", parse_ms);
    println!(
        "    - read:          {:.3}ms",
        parse_diagnostics.timings.read_ms
    );
    println!(
        "    - parse-sections:{:.3}ms",
        parse_diagnostics.timings.parse_sections_ms
    );
    println!(
        "    - build-doc:     {:.3}ms",
        parse_diagnostics.timings.build_document_ms
    );
    println!("  Validation:        {:.3}ms", validation_ms);
    println!(
        "  Validation result: {}",
        if validation_error.is_none() {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!();

    // Multi-document: show each document
    if !doc.is_single_document() {
        let total_docs = doc.document_count();
        println!("DOCUMENTS: {}", total_docs);
        println!();

        for (doc_idx, d) in doc.iter_chain().enumerate() {
            println!(
                "DOCUMENT {} [lines {}-{}]",
                doc_idx + 1,
                d.sections.first().map(|s| s.start_line + 1).unwrap_or(0),
                d.sections.last().map(|s| s.end_line + 1).unwrap_or(0)
            );
            println!("{}", "=".repeat(56));
            print_ast_overview(d);
            println!();
            print_structure(d);
            println!();
            print_variables(d);
            println!();
            let w = Workflow::from_document_with_analysis(d);
            print_logic_flow(d, &w);
            println!();
        }

        println!("WARNINGS & HINTS");
        println!("----------------");
        for (doc_idx, d) in doc.iter_chain().enumerate() {
            print_warnings_for_doc(d, doc_idx + 1);
        }
        println!();
        println!("SUMMARY");
        println!("-------");
        match validation_error {
            Some(e) => println!("  FAILED: {}", e),
            None => println!("  OK - No issues found. Test appears structurally valid."),
        }
        return;
    }

    // Single document: original format
    println!("AST OVERVIEW");
    println!("------------");
    print_ast_overview(doc);
    println!();

    println!("STRUCTURE");
    println!("---------");
    print_structure(doc);
    println!();

    print_effective_runtime(effective_runtime.as_ref());
    println!();

    println!("VARIABLES");
    println!("---------");
    print_variables(doc);
    println!();

    println!("LOGIC FLOW");
    println!("----------");
    print_logic_flow(doc, workflow);
    println!();

    println!("SOURCE CONFIGURATION");
    println!("--------------------");
    print_source_info(doc, file_path);
    println!();

    println!("WARNINGS & HINTS");
    println!("----------------");
    print_warnings(doc);
    println!();

    println!("SUMMARY");
    println!("-------");
    match validation_error {
        Some(e) => println!("  FAILED: {}", e),
        None => println!("  OK - No issues found. Test appears structurally valid."),
    }
}

fn print_effective_runtime(effective: Option<&EffectiveRuntimeOptions>) {
    println!("EFFECTIVE RUNTIME");
    println!("-----------------");
    match effective {
        Some(v) => {
            println!(
                "  timeout: {}s ({:?})",
                v.timeout_seconds.value, v.timeout_seconds.source
            );
            println!("  retry: {} ({:?})", v.retry.value, v.retry.source);
            println!(
                "  retry_delay: {}s ({:?})",
                v.retry_delay_seconds.value, v.retry_delay_seconds.source
            );
            println!("  no_retry: {} ({:?})", v.no_retry.value, v.no_retry.source);
            println!(
                "  compression: {} ({:?})",
                v.compression.value, v.compression.source
            );
        }
        None => {
            println!("  unavailable (runtime options failed to resolve)");
        }
    }
}

fn print_ast_overview(doc: &parser::GctfDocument) {
    println!("  #   Section          Lines         Type           Raw Size");
    println!("  ---------------------------------------------------------------");
    for (i, section) in doc.sections.iter().enumerate() {
        let content_kind = match &section.content {
            SectionContent::Single(_) => "single",
            SectionContent::Json(_) => "json",
            SectionContent::JsonLines(_) => "json-lines",
            SectionContent::KeyValues(_) => "key-values",
            SectionContent::Assertions(_) => "assertions",
            SectionContent::Extract(_) => "extract",
            SectionContent::Meta(_) => "meta",
            SectionContent::Empty => "empty",
        };
        let raw_bytes = section.raw_content.len();
        println!(
            "  {:<2}  {:<16} {:>3}-{:>3}        {:<12}   {:>6} bytes",
            i + 1,
            section.section_type.as_str(),
            section.start_line + 1,
            section.end_line + 1,
            content_kind,
            raw_bytes
        );
        if let SectionContent::Meta(meta) = &section.content {
            if let Some(name) = &meta.name {
                println!("       name: {}", name);
            }
            if let Some(summary) = &meta.summary {
                println!("       summary: {}", summary);
            }
            if !meta.tags.is_empty() {
                println!("       tags: {:?}", meta.tags);
            }
            if let Some(owner) = &meta.owner {
                println!("       owner: {}", owner);
            }
            if !meta.links.is_empty() {
                println!("       links: {:?}", meta.links);
            }
        }
    }
}

fn print_structure(doc: &parser::GctfDocument) {
    let has_endpoint = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Endpoint);
    let has_request = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Request);
    let has_response = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Response);
    let has_error = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Error);
    let has_extract = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Extract);
    let has_asserts = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::Asserts);
    let has_request_headers = doc
        .sections
        .iter()
        .any(|s| s.section_type == SectionType::RequestHeaders);

    if has_endpoint {
        println!("  [OK] ENDPOINT section present");
    }
    if has_request {
        println!("  [OK] REQUEST section present");
    }
    if has_response {
        println!("  [OK] RESPONSE section present");
    }
    if has_error {
        println!("  [OK] ERROR section present (testing error handling)");
    }
    if has_extract {
        println!("  [OK] EXTRACT section present (variable extraction)");
    }
    if has_asserts {
        println!("  [OK] ASSERTS section present (custom assertions)");
    }
    if has_request_headers {
        println!("  [OK] REQUEST_HEADERS section present");
    }

    if !has_endpoint {
        println!("  [ERROR] Missing ENDPOINT section");
    }
    if !has_request && !has_error {
        println!("  [WARN] No REQUEST or ERROR section");
    }
}

fn print_variables(doc: &parser::GctfDocument) {
    let extract_sections: Vec<_> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .collect();

    if extract_sections.is_empty() {
        println!("  No variables defined or used.");
        return;
    }

    for section in extract_sections {
        println!(
            "  Extract block at lines {}-{}:",
            section.start_line + 1,
            section.end_line + 1
        );
        for line in section.raw_content.lines() {
            if let Some((name, type_opt, value)) =
                crate::parser::gctf_tokenizer::tokenize_extract_line_full(line)
            {
                if let Some(tn) = type_opt {
                    println!("    ${{ {:<15} }} :{} = {}", name, tn, value);
                } else {
                    println!("    ${{ {:<15} }} = {}", name, value);
                }
            }
        }
    }

    // Check for variable usage
    let mut var_usages = Vec::new();
    for section in &doc.sections {
        if let SectionContent::Json(value) = &section.content {
            let json_str = value.to_string();
            if json_str.contains("{{ ") && json_str.contains(" }}") {
                var_usages.push((section.section_type.as_str(), section.start_line));
            }
        }
    }

    if !var_usages.is_empty() {
        println!("  Variable usages:");
        for (section_type, line) in var_usages {
            println!("    - {} section at line {}", section_type, line + 1);
        }
    }
}

fn print_logic_flow(doc: &parser::GctfDocument, workflow: &Workflow) {
    let total_errors = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Error)
        .count();
    let total_requests = workflow.summary.total_requests;
    let total_responses = workflow.summary.total_responses;
    let total_extractions = workflow.summary.total_extractions;
    let total_assertions = workflow.summary.total_assertions;

    println!("  {}", workflow.rpc_mode_name());

    if total_errors > 0 && total_requests == 1 {
        println!("  Pattern: Single request -> gRPC error response");
    } else if total_requests == 1 && total_responses == 1 {
        println!("  Pattern: Single request -> Single response");
    } else if total_requests == 1 && total_responses > 1 {
        println!("  Pattern: Single request -> {} responses", total_responses);
    } else if total_requests > 1 && total_responses == 1 {
        println!("  Pattern: {} requests -> Single response", total_requests);
    } else if total_requests > 1 && total_responses > 1 {
        println!(
            "  Pattern: {} requests, {} responses (full duplex)",
            total_requests, total_responses
        );
    } else if total_requests > 1 && total_errors > 0 {
        println!(
            "  Pattern: {} requests with {} error(s)",
            total_requests, total_errors
        );
    }

    println!(
        "  Steps: {}",
        total_requests + total_responses + total_errors + total_extractions + total_assertions + 1
    );

    if total_extractions > 0 {
        println!("  Variables: {} extraction(s)", total_extractions);
    }
    if total_assertions > 0 {
        println!("  Assertions: {} block(s)", total_assertions);
    }
    if workflow.has_streaming() {
        println!("  Streaming: enabled");
    }

    let with_asserts_count = doc
        .sections
        .iter()
        .filter(|s| {
            matches!(s.section_type, SectionType::Response | SectionType::Error)
                && s.inline_options.with_asserts
        })
        .count();
    if with_asserts_count > 0 {
        println!(
            "  With Asserts: {} response/error section(s)",
            with_asserts_count
        );
    }

    let unordered_count = doc
        .sections
        .iter()
        .filter(|s| s.inline_options.unordered_arrays)
        .count();
    if unordered_count > 0 {
        println!("  Unordered Arrays: {} section(s)", unordered_count);
    }

    let partial_count = doc
        .sections
        .iter()
        .filter(|s| s.inline_options.partial)
        .count();
    if partial_count > 0 {
        println!("  Partial Matching: {} section(s)", partial_count);
    }

    if let Some(options) = doc.get_options()
        && !options.is_empty()
    {
        let mut sorted: Vec<_> = options.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        println!("  OPTIONS Overrides:");
        for (key, value) in sorted {
            println!("    - {}: {}", key, value);
        }
    }

    for section in &doc.sections {
        if section.section_type == SectionType::Bench
            && let SectionContent::KeyValues(bench) = &section.content
        {
            let mut sorted: Vec<_> = bench.iter().collect();
            sorted.sort_by(|a, b| {
                bench_key_rank(a.0)
                    .cmp(&bench_key_rank(b.0))
                    .then_with(|| a.0.cmp(b.0))
            });
            println!("  BENCH Profile:");
            for (key, value) in sorted {
                println!("    - {}: {}", key, value);
            }
        }
    }

    print_bench_resolved(doc);
}

fn print_warnings(doc: &parser::GctfDocument) {
    print_warnings_for_doc(doc, 0);
}

fn print_warnings_for_doc(doc: &parser::GctfDocument, _doc_num: usize) {
    let sections = &doc.sections;
    let mut has_warnings = false;

    let has_endpoint = sections
        .iter()
        .any(|s| s.section_type == SectionType::Endpoint);
    if !has_endpoint {
        println!("  [ERROR] No ENDPOINT section found");
        has_warnings = true;
    }

    let has_request = sections
        .iter()
        .any(|s| s.section_type == SectionType::Request);
    let has_error = sections
        .iter()
        .any(|s| s.section_type == SectionType::Error);
    if !has_request && !has_error {
        println!("  [WARN] No REQUEST or ERROR section found");
        has_warnings = true;
    }

    let has_response = sections
        .iter()
        .any(|s| s.section_type == SectionType::Response);
    if has_request && !has_response && !has_error {
        println!("  [WARN] REQUEST section present but no RESPONSE or ERROR section");
        has_warnings = true;
    }

    // Semantic analysis
    let workflow = Workflow::from_document_with_analysis(doc);
    for event in workflow.semantic_analysis() {
        if let crate::execution::WorkflowEvent::SemanticAnalysis {
            type_mismatches,
            unknown_plugins,
        } = event
        {
            for mismatch in type_mismatches {
                println!("  [ERROR] Line {}: {}", mismatch.line, mismatch.message);
                println!(
                    "         Expression: {}",
                    mismatch.expression.as_ref().unwrap_or(&"".to_string())
                );
                has_warnings = true;
            }
            for unknown in unknown_plugins {
                println!("  [ERROR] Line {}: {}", unknown.line, unknown.message);
                println!(
                    "         Assertion: {}",
                    unknown.expression.as_ref().unwrap_or(&"".to_string())
                );
                has_warnings = true;
            }
        }
    }

    for hint in workflow_optimizer_hints(&workflow) {
        println!(
            "  [HINT] Line {}: [{}] {} -> {}",
            hint.line, hint.rule_id, hint.before, hint.after
        );
        println!("         Boolean expression compared with true/false can be simplified");
        has_warnings = true;
    }

    for (i, section) in sections.iter().enumerate() {
        if section.inline_options.with_asserts {
            let has_following_asserts = sections
                .get(i + 1)
                .is_some_and(|s| s.section_type == SectionType::Asserts);
            if !has_following_asserts {
                println!(
                    "  [WARN] Line {}: with_asserts option set but no",
                    section.start_line + 1
                );
                println!("         ASSERTS section follows");
                has_warnings = true;
            }
        }

        if section.section_type == SectionType::Request
            && matches!(section.content, SectionContent::Empty)
        {
            println!(
                "  [INFO] Line {}: Empty REQUEST section will send",
                section.start_line + 1
            );
            println!("         empty JSON object {{}}");
            has_warnings = true;
        }

        if section.section_type == SectionType::Extract
            && let SectionContent::Extract(extractions) = &section.content
            && extractions.is_empty()
        {
            println!(
                "  [WARN] Line {}: EXTRACT section has no variables",
                section.start_line + 1
            );
            has_warnings = true;
        }

        if section.section_type == SectionType::Asserts
            && let SectionContent::Assertions(assertions) = &section.content
            && assertions.is_empty()
        {
            println!(
                "  [WARN] Line {}: ASSERTS section has no assertions",
                section.start_line + 1
            );
            has_warnings = true;
        }
    }

    if !has_warnings {
        println!("  [OK] No structural warnings or hints.");
    }
}

fn print_source_info(doc: &parser::GctfDocument, file_path: &Path) {
    use crate::bench::sources::SourceDefinition;
    use crate::bench::sources::analyzer::SourceUsageAnalyzer;
    use crate::bench::sources::index::read_index_key_type;
    use crate::bench::sources::index_builder::index_path_for_source;

    let bench_section = match doc
        .sections
        .iter()
        .find(|s| s.section_type == SectionType::Bench)
    {
        Some(s) => s,
        None => return,
    };

    let bench = match &bench_section.content {
        SectionContent::KeyValues(kv) => kv,
        _ => return,
    };

    let raw = match bench.get("sources") {
        Some(r) if !r.trim().is_empty() => r,
        _ => return,
    };

    let sources: Vec<SourceDefinition> = match serde_yaml_ng::from_str(raw) {
        Ok(s) => s,
        Err(_) => return,
    };

    let plan = SourceUsageAnalyzer::analyze(doc, &sources);

    if sources.is_empty() {
        return;
    }

    println!("SOURCES");
    println!("-------");
    for s in &sources {
        let source_file_path = s.file.as_str();
        let resolved_path = FileUtils::resolve_relative_path(file_path, source_file_path);
        let columns = s.indexed_columns();
        let key_column = columns.first().copied().unwrap_or("");

        let idx_path = index_path_for_source(&resolved_path, key_column);
        let idx_exists = idx_path.exists();
        let (status, type_display) = if idx_exists {
            let idx_meta = std::fs::metadata(&idx_path).ok();
            let src_meta = std::fs::metadata(&resolved_path).ok();
            let fresh = match (idx_meta, src_meta) {
                (Some(i), Some(s)) => i
                    .modified()
                    .ok()
                    .zip(s.modified().ok())
                    .map(|(it, st)| it >= st)
                    .unwrap_or(false),
                _ => false,
            };
            let type_from_idx = std::fs::File::open(&idx_path)
                .ok()
                .and_then(|mut f| read_index_key_type(&mut f).ok())
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|| "?".to_string());
            (if fresh { "✓ fresh" } else { "⚠ stale" }, type_from_idx)
        } else {
            ("✗ missing", "?".to_string())
        };

        let name_display = s.name.as_deref().unwrap_or("(unnamed)");
        let indexed_by_display = if columns.is_empty() {
            "(none)".to_string()
        } else {
            columns.join(", ")
        };
        println!(
            "  - {}: file={}, indexed_by=[{}], type={}, index={}",
            name_display, s.file, indexed_by_display, type_display, status
        );
    }

    if !plan.required_indexes.is_empty() {
        println!();
        println!("INDEX REQUIREMENTS (used by templates)");
        println!("--------------------------------------");
        for req in &plan.required_indexes {
            println!(
                "  - {}.{} (reason: {:?})",
                req.source, req.column, req.reason
            );
        }
    }
}

fn bench_section_map(doc: &parser::GctfDocument) -> Option<&HashMap<String, String>> {
    doc.sections.iter().find_map(|section| {
        if section.section_type == SectionType::Bench
            && let SectionContent::KeyValues(bench) = &section.content
        {
            return Some(bench);
        }
        None
    })
}

fn print_bench_resolved(doc: &parser::GctfDocument) {
    let Some(bench_section) = bench_section_map(doc) else {
        return;
    };

    match BenchConfigResolved::from_bench_section(Some(bench_section)) {
        Ok(config) => {
            println!("  BENCH Resolved (value + source):");
            for (internal_key, output_key) in [
                ("concurrency", "concurrency"),
                ("load_schedule", "load_schedule"),
                ("load_start", "load_start"),
                ("load_step", "load_step"),
                ("load_end", "load_end"),
                ("load_step_duration", "load_step_duration"),
                ("load_max_duration", "load_max_duration"),
                ("progress_interval", "progress_interval"),
            ] {
                let source = config
                    .option_sources
                    .get(internal_key)
                    .copied()
                    .unwrap_or(crate::commands::bench::BenchOptionSource::Default)
                    .as_str();
                let value = match internal_key {
                    "concurrency" => config.concurrency.to_string(),
                    "load_schedule" => config.load_schedule.clone(),
                    "load_start" => config
                        .load_start
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "<none>".to_string()),
                    "load_step" => config
                        .load_step
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "<none>".to_string()),
                    "load_end" => config
                        .load_end
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "<none>".to_string()),
                    "load_step_duration" => config
                        .load_step_duration
                        .map(|v| format!("{}ms", v.as_millis()))
                        .unwrap_or_else(|| "<none>".to_string()),
                    "load_max_duration" => config
                        .load_max_duration
                        .map(|v| format!("{}ms", v.as_millis()))
                        .unwrap_or_else(|| "<none>".to_string()),
                    "progress_interval" => format!("{}ms", config.progress_interval.as_millis()),
                    _ => "<n/a>".to_string(),
                };
                println!("    - {}: {} (source: {})", output_key, value, source);
            }
        }
        Err(err) => {
            println!("  [WARN] Unable to resolve BENCH defaults/sources: {}", err);
        }
    }
}
