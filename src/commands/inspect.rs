// Inspect command - show detailed AST analysis and structure

use anyhow::Result;
use std::path::Path;

use crate::cli::args::InspectArgs;
use crate::execution;
use crate::optimizer;
use crate::parser;
use crate::report::{AstOverview, InspectReport, SectionInfo};
use crate::semantics;

pub async fn handle_inspect(args: &InspectArgs) -> Result<()> {
    let file_path = &args.file;
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_start = std::time::Instant::now();

    // Use error recovery parsing to show all errors
    let parse_result = parser::parse_with_recovery(file_path);
    let doc = parse_result.document;
    let diagnostics = parse_result.diagnostics;
    let parse_diagnostics = parser::parse_gctf_with_diagnostics(file_path)
        .ok()
        .map(|(_, d)| d)
        .unwrap_or_default();

    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    // Show any parse diagnostics
    if !diagnostics.is_empty() {
        eprintln!();
        eprintln!("PARSE DIAGNOSTICS");
        eprintln!("=================");
        eprintln!("File: {}", file_path.display());
        eprintln!("Recovered sections: {}", parse_result.recovered_sections);
        eprintln!("Failed sections: {}", parse_result.failed_sections);
        eprintln!();

        for diagnostic in &diagnostics.diagnostics {
            print_diagnostic(diagnostic);
            eprintln!();
        }
    }

    let validation_start = std::time::Instant::now();
    let validation_result = parser::validate_document(&doc);
    let validation_ms = validation_start.elapsed().as_secs_f64() * 1000.0;

    if args.is_json() {
        let mut inspect_diagnostics: Vec<crate::report::Diagnostic> = Vec::new();
        let mut semantic_diagnostics: Vec<crate::report::Diagnostic> = Vec::new();
        let mut optimization_hints: Vec<crate::report::Diagnostic> = Vec::new();
        let file_str = file_path.to_string_lossy().to_string();

        if let Err(e) = &validation_result {
            inspect_diagnostics.push(crate::report::Diagnostic::error(
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
                }
            }
        }

        for mismatch in semantics::collect_assertion_type_mismatches(&doc) {
            let diag = crate::report::Diagnostic::error(
                &file_str,
                &mismatch.rule_id,
                &mismatch.message,
                mismatch.line,
            )
            .with_hint(&format!("Expression: {}", mismatch.expression));
            semantic_diagnostics.push(diag.clone());
            inspect_diagnostics.push(diag);
        }

        for unknown in semantics::collect_unknown_plugin_calls(&doc) {
            let message = match &unknown.suggestion {
                Some(suggestion) => format!("{} (use {})", unknown.message, suggestion),
                None => unknown.message.clone(),
            };
            let diag = crate::report::Diagnostic::error(
                &file_str,
                &unknown.rule_id,
                &message,
                unknown.line,
            )
            .with_hint(&format!("Assertion: {}", unknown.expression));
            semantic_diagnostics.push(diag.clone());
            inspect_diagnostics.push(diag);
        }

        for hint in optimizer::collect_assertion_optimizations(&doc) {
            let diag = crate::report::Diagnostic::hint(
                &file_str,
                &hint.rule_id,
                &format!("Safe rewrite available: {} -> {}", hint.before, hint.after),
                hint.line,
            )
            .with_hint("Boolean expression compared with true/false can be simplified");
            optimization_hints.push(diag.clone());
            inspect_diagnostics.push(diag);
        }

        // Build sections info
        let sections_info: Vec<SectionInfo> = doc
            .sections
            .iter()
            .map(|s| {
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
            })
            .collect();

        // Infer RPC mode from execution plan
        let execution_plan = execution::ExecutionPlan::from_document(&doc);
        let inferred_rpc_mode = Some(execution_plan.summary.rpc_mode_name);

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
            inferred_rpc_mode,
        };

        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        // Print detailed technical AST analysis
        print_detailed_analysis(
            &doc,
            file_path,
            &parse_diagnostics,
            parse_ms,
            validation_ms,
            validation_result.err().map(|e| e.to_string()).as_deref(),
        );
    }

    Ok(())
}

fn print_diagnostic(diagnostic: &crate::diagnostics::Diagnostic) {
    use crate::diagnostics::DiagnosticSeverity;

    let severity_str = match diagnostic.severity {
        DiagnosticSeverity::Error => "ERROR",
        DiagnosticSeverity::Warning => "WARNING",
        DiagnosticSeverity::Information => "INFO",
        DiagnosticSeverity::Hint => "HINT",
    };

    eprintln!(
        "[{}] {}: {}",
        severity_str,
        diagnostic.code.as_str(),
        diagnostic.message
    );

    if let Some(context) = &diagnostic.context {
        eprintln!("  {}", context);
    }

    if !diagnostic.suggestions.is_empty() {
        eprintln!();
        eprintln!("Suggestions:");
        for suggestion in &diagnostic.suggestions {
            eprintln!("  - {}", suggestion);
        }
    }
}

fn print_detailed_analysis(
    doc: &parser::GctfDocument,
    file_path: &Path,
    parse_diagnostics: &parser::ParseDiagnostics,
    parse_ms: f64,
    validation_ms: f64,
    validation_error: Option<&str>,
) {
    println!();
    println!("ANALYSIS REPORT");
    println!("===============");
    println!();
    println!("FILE: {}", file_path.display());
    println!();

    // Parse Profiling
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

    // AST Overview
    println!("AST OVERVIEW");
    println!("------------");
    println!("  #   Section          Lines         Type           Raw Size");
    println!("  ---------------------------------------------------------------");
    for (i, section) in doc.sections.iter().enumerate() {
        let content_kind = match &section.content {
            parser::ast::SectionContent::Single(_) => "single",
            parser::ast::SectionContent::Json(_) => "json",
            parser::ast::SectionContent::JsonLines(_) => "json-lines",
            parser::ast::SectionContent::KeyValues(_) => "key-values",
            parser::ast::SectionContent::Assertions(_) => "assertions",
            parser::ast::SectionContent::Extract(_) => "extract",
            parser::ast::SectionContent::Empty => "empty",
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
    }
    println!();

    // Structure
    println!("STRUCTURE");
    println!("---------");
    let has_endpoint = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Endpoint);
    let has_request = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Request);
    let has_response = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Response);
    let has_error = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Error);
    let has_extract = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Extract);
    let has_asserts = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Asserts);
    let has_request_headers = doc
        .sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::RequestHeaders);

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
    println!();

    // Variables
    println!("VARIABLES");
    println!("---------");
    let extract_sections: Vec<_> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == parser::ast::SectionType::Extract)
        .collect();

    if extract_sections.is_empty() {
        println!("  No variables defined or used.");
    } else {
        for section in extract_sections {
            if let parser::ast::SectionContent::Extract(extractions) = &section.content {
                println!(
                    "  Extract block at lines {}-{}:",
                    section.start_line + 1,
                    section.end_line + 1
                );
                for (name, query) in extractions {
                    println!("    ${{ {:<15} }} = {}", name, query);
                }
            }
        }

        // Check for variable usage
        let mut var_usages = Vec::new();
        for section in &doc.sections {
            if let parser::ast::SectionContent::Json(value) = &section.content {
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
    println!();

    // Logic Flow
    println!("LOGIC FLOW");
    println!("----------");
    print_logic_flow(doc);
    println!();

    // Warnings/Hints
    println!("WARNINGS & HINTS");
    println!("----------------");
    let sections = &doc.sections;
    let mut has_warnings = false;

    // Check for missing required sections
    let has_endpoint = sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Endpoint);
    if !has_endpoint {
        println!("  [ERROR] No ENDPOINT section found");
        has_warnings = true;
    }

    // Check for REQUEST/ERROR sections
    let has_request = sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Request);
    let has_error = sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Error);
    if !has_request && !has_error {
        println!("  [WARN] No REQUEST or ERROR section found");
        has_warnings = true;
    }

    // Check for RESPONSE sections
    let has_response = sections
        .iter()
        .any(|s| s.section_type == parser::ast::SectionType::Response);
    if has_request && !has_response && !has_error {
        println!("  [WARN] REQUEST section present but no RESPONSE or ERROR section");
        has_warnings = true;
    }

    // Check each section for issues
    let type_mismatches = semantics::collect_assertion_type_mismatches(doc);
    let unknown_plugins = semantics::collect_unknown_plugin_calls(doc);

    for (i, section) in sections.iter().enumerate() {
        // Check for `with_asserts` without following ASSERTS
        if section.inline_options.with_asserts {
            let has_following_asserts = sections[i + 1..]
                .iter()
                .take_while(|s| s.section_type != parser::ast::SectionType::Request)
                .any(|s| s.section_type == parser::ast::SectionType::Asserts);
            if !has_following_asserts {
                println!(
                    "  [WARN] Line {}: with_asserts option set but no",
                    section.start_line + 1
                );
                println!("         ASSERTS section follows");
                has_warnings = true;
            }
        }

        // Check for empty REQUEST sections
        if section.section_type == parser::ast::SectionType::Request
            && matches!(section.content, parser::ast::SectionContent::Empty)
        {
            println!(
                "  [INFO] Line {}: Empty REQUEST section will send",
                section.start_line + 1
            );
            println!("         empty JSON object {{}}");
            has_warnings = true;
        }

        // Check for unused EXTRACT variables
        if section.section_type == parser::ast::SectionType::Extract
            && let parser::ast::SectionContent::Extract(extractions) = &section.content
            && extractions.is_empty()
        {
            println!(
                "  [WARN] Line {}: EXTRACT section has no variables",
                section.start_line + 1
            );
            has_warnings = true;
        }

        // Check for empty ASSERTS sections
        if section.section_type == parser::ast::SectionType::Asserts
            && let parser::ast::SectionContent::Assertions(assertions) = &section.content
            && assertions.is_empty()
        {
            println!(
                "  [WARN] Line {}: ASSERTS section has no assertions",
                section.start_line + 1
            );
            has_warnings = true;
        }
    }

    for mismatch in &type_mismatches {
        println!("  [ERROR] Line {}: {}", mismatch.line, mismatch.message);
        println!("         Expression: {}", mismatch.expression);
        has_warnings = true;
    }

    for unknown in &unknown_plugins {
        println!("  [ERROR] Line {}: {}", unknown.line, unknown.message);
        println!("         Assertion: {}", unknown.expression);
        has_warnings = true;
    }

    for hint in optimizer::collect_assertion_optimizations(doc) {
        println!(
            "  [HINT] Line {}: [{}] {} -> {}",
            hint.line, hint.rule_id, hint.before, hint.after
        );
        println!("         Boolean expression compared with true/false can be simplified");
        has_warnings = true;
    }

    if !has_warnings {
        println!("  [OK] No structural warnings or hints.");
    }
    println!();

    // Summary
    println!("SUMMARY");
    println!("-------");
    match validation_error {
        Some(e) => println!("  FAILED: {}", e),
        None => {
            if unknown_plugins.is_empty() && type_mismatches.is_empty() {
                println!("  OK - No issues found. Test appears structurally valid.");
            } else {
                println!(
                    "  FAILED: semantic validation failed ({} unknown plugin call(s), {} type mismatch(es))",
                    unknown_plugins.len(),
                    type_mismatches.len()
                );
            }
        }
    }
}

fn print_logic_flow(doc: &parser::GctfDocument) {
    use crate::execution::{get_call_type, get_workflow_summary};

    let summary = get_workflow_summary(&doc.sections);
    let call_type = get_call_type(&summary);

    println!("  {}", call_type);

    // Build pattern description
    if summary.total_errors > 0 && summary.total_requests == 1 {
        println!("  Pattern: Single request -> gRPC error response");
    } else if summary.total_requests == 1 && summary.total_responses == 1 {
        println!("  Pattern: Single request -> Single response");
    } else if summary.total_requests == 1 && summary.total_responses > 1 {
        println!(
            "  Pattern: Single request -> {} responses",
            summary.total_responses
        );
    } else if summary.total_requests > 1 && summary.total_responses == 1 {
        println!(
            "  Pattern: {} requests -> Single response",
            summary.total_requests
        );
    } else if summary.total_requests > 1 && summary.total_responses > 1 {
        println!(
            "  Pattern: {} requests, {} responses (full duplex)",
            summary.total_requests, summary.total_responses
        );
    } else if summary.total_requests > 1 && summary.total_errors > 0 {
        println!(
            "  Pattern: {} requests with {} error(s)",
            summary.total_requests, summary.total_errors
        );
    }

    // Show workflow summary
    println!(
        "  Steps: {}",
        summary.total_requests
            + summary.total_responses
            + summary.total_errors
            + summary.total_extractions
            + summary.total_assertions
            + 1
    );

    // Show additional info
    if summary.total_extractions > 0 {
        println!("  Variables: {} extraction(s)", summary.total_extractions);
    }
    if summary.total_assertions > 0 {
        println!("  Assertions: {} block(s)", summary.total_assertions);
    }
    if summary.has_streaming {
        println!("  Streaming: enabled");
    }

    // Show inline options summary
    let with_asserts_count = doc
        .sections
        .iter()
        .filter(|s| {
            s.section_type == parser::ast::SectionType::Response && s.inline_options.with_asserts
        })
        .count();
    if with_asserts_count > 0 {
        println!("  With Asserts: {} response(s)", with_asserts_count);
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
}
