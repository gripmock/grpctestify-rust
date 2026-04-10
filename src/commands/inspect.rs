// Inspect command - show detailed AST analysis and structure

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::cli::args::InspectArgs;
use crate::execution::{ExecutionPlan, Workflow};
use crate::parser;
use crate::parser::ast::{SectionContent, SectionType};
use crate::report::{AstOverview, InspectReport, SectionInfo};

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
    let validation_result = parser::validate_document(&doc);
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
            validation_result.err().map(|e| e.to_string()).as_deref(),
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
        if let Err(e) = parser::validate_document(d) {
            let msg = if doc.is_single_document() {
                e.to_string()
            } else {
                format!("Document {}: {}", doc_idx + 1, e)
            };
            inspect_diagnostics.push(crate::report::Diagnostic::error(
                &file_str, "VALIDATION_ERROR", &msg, 1,
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
        }
        let w = Workflow::from_document_with_analysis(d);
        for event in w.semantic_analysis() {
            if let crate::execution::WorkflowEvent::SemanticAnalysis {
                type_mismatches,
                unknown_plugins,
            } = event
            {
                for mismatch in type_mismatches {
                    semantic_diagnostics.push(
                        crate::report::Diagnostic::error(
                            &file_str, &mismatch.rule_id, &mismatch.message, mismatch.line,
                        )
                        .with_hint(&mismatch.expression.as_ref().map(|e| format!("Expression: {}", e)).unwrap_or_default()),
                    );
                }
                for unknown in unknown_plugins {
                    semantic_diagnostics.push(
                        crate::report::Diagnostic::error(
                            &file_str, &unknown.rule_id, &unknown.message, unknown.line,
                        )
                        .with_hint(&unknown.expression.as_ref().map(|e| format!("Assertion: {}", e)).unwrap_or_default()),
                    );
                }
            }
        }
        for hint in crate::optimizer::collect_assertion_optimizations(d) {
            optimization_hints.push(
                crate::report::Diagnostic::hint(&file_str, &hint.rule_id,
                    &format!("Safe rewrite available: {} -> {}", hint.before, hint.after), hint.line)
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
    let report = InspectReport {
        file: file_str,
        parse_time_ms: parse_ms,
        validation_time_ms: validation_ms,
        ast: AstOverview { sections: sections_info },
        diagnostics: inspect_diagnostics,
        semantic_diagnostics,
        optimization_hints,
        inferred_rpc_mode: Some(plan.summary.rpc_mode_name.clone()),
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

fn section_to_info(section: &parser::ast::Section) -> SectionInfo {
    let content_kind = match &section.content {
        SectionContent::Single(_) => "single",
        SectionContent::Json(_) => "json",
        SectionContent::JsonLines(_) => "json-lines",
        SectionContent::KeyValues(_) => "key-values",
        SectionContent::Assertions(_) => "assertions",
        SectionContent::Extract(_) => "extract",
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
    _workflow: &Workflow,
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
        if validation_error.is_none() { "OK" } else { "FAILED" }
    );
    println!();

    // Multi-document: show each document
    if !doc.is_single_document() {
        let total_docs = doc.document_count();
        println!("DOCUMENTS: {}", total_docs);
        println!();

        for (doc_idx, d) in doc.iter_chain().enumerate() {
            println!("DOCUMENT {} [lines {}-{}]", doc_idx + 1,
                d.sections.first().map(|s| s.start_line + 1).unwrap_or(0),
                d.sections.last().map(|s| s.end_line + 1).unwrap_or(0));
            println!("{}", "=".repeat(56));
            print_ast_overview(d);
            println!();
            print_structure(d);
            println!();
            print_variables(d);
            println!();
            print_logic_flow(d);
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

    println!("VARIABLES");
    println!("---------");
    print_variables(doc);
    println!();

    println!("LOGIC FLOW");
    println!("----------");
    print_logic_flow(doc);
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
    }
}

fn print_structure(doc: &parser::GctfDocument) {
    let has_endpoint = doc.sections.iter().any(|s| s.section_type == SectionType::Endpoint);
    let has_request = doc.sections.iter().any(|s| s.section_type == SectionType::Request);
    let has_response = doc.sections.iter().any(|s| s.section_type == SectionType::Response);
    let has_error = doc.sections.iter().any(|s| s.section_type == SectionType::Error);
    let has_extract = doc.sections.iter().any(|s| s.section_type == SectionType::Extract);
    let has_asserts = doc.sections.iter().any(|s| s.section_type == SectionType::Asserts);
    let has_request_headers = doc.sections.iter().any(|s| s.section_type == SectionType::RequestHeaders);

    if has_endpoint { println!("  [OK] ENDPOINT section present"); }
    if has_request { println!("  [OK] REQUEST section present"); }
    if has_response { println!("  [OK] RESPONSE section present"); }
    if has_error { println!("  [OK] ERROR section present (testing error handling)"); }
    if has_extract { println!("  [OK] EXTRACT section present (variable extraction)"); }
    if has_asserts { println!("  [OK] ASSERTS section present (custom assertions)"); }
    if has_request_headers { println!("  [OK] REQUEST_HEADERS section present"); }

    if !has_endpoint { println!("  [ERROR] Missing ENDPOINT section"); }
    if !has_request && !has_error { println!("  [WARN] No REQUEST or ERROR section"); }
}

fn print_variables(doc: &parser::GctfDocument) {
    let extract_sections: Vec<_> = doc.sections.iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .collect();

    if extract_sections.is_empty() {
        println!("  No variables defined or used.");
        return;
    }

    for section in extract_sections {
        if let SectionContent::Extract(extractions) = &section.content {
            println!(
                "  Extract block at lines {}-{}:",
                section.start_line + 1, section.end_line + 1
            );
            for (name, query) in sorted_kv(extractions) {
                println!("    ${{ {:<15} }} = {}", name, query);
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

fn sorted_kv(map: &HashMap<String, String>) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    pairs.sort_by(|(ka, _), (kb, _)| ka.cmp(kb));
    pairs
}

fn print_logic_flow(doc: &parser::GctfDocument) {
    use crate::execution::{get_call_type, get_workflow_summary};

    let summary = get_workflow_summary(&doc.sections);
    let call_type = get_call_type(&summary);
    println!("  {}", call_type);

    if summary.total_errors > 0 && summary.total_requests == 1 {
        println!("  Pattern: Single request -> gRPC error response");
    } else if summary.total_requests == 1 && summary.total_responses == 1 {
        println!("  Pattern: Single request -> Single response");
    } else if summary.total_requests == 1 && summary.total_responses > 1 {
        println!("  Pattern: Single request -> {} responses", summary.total_responses);
    } else if summary.total_requests > 1 && summary.total_responses == 1 {
        println!("  Pattern: {} requests -> Single response", summary.total_requests);
    } else if summary.total_requests > 1 && summary.total_responses > 1 {
        println!("  Pattern: {} requests, {} responses (full duplex)", summary.total_requests, summary.total_responses);
    } else if summary.total_requests > 1 && summary.total_errors > 0 {
        println!("  Pattern: {} requests with {} error(s)", summary.total_requests, summary.total_errors);
    }

    println!(
        "  Steps: {}",
        summary.total_requests + summary.total_responses + summary.total_errors
            + summary.total_extractions + summary.total_assertions + 1
    );

    if summary.total_extractions > 0 {
        println!("  Variables: {} extraction(s)", summary.total_extractions);
    }
    if summary.total_assertions > 0 {
        println!("  Assertions: {} block(s)", summary.total_assertions);
    }
    if summary.has_streaming {
        println!("  Streaming: enabled");
    }

    let with_asserts_count = doc.sections.iter().filter(|s|
        s.section_type == SectionType::Response && s.inline_options.with_asserts).count();
    if with_asserts_count > 0 { println!("  With Asserts: {} response(s)", with_asserts_count); }

    let unordered_count = doc.sections.iter().filter(|s| s.inline_options.unordered_arrays).count();
    if unordered_count > 0 { println!("  Unordered Arrays: {} section(s)", unordered_count); }

    let partial_count = doc.sections.iter().filter(|s| s.inline_options.partial).count();
    if partial_count > 0 { println!("  Partial Matching: {} section(s)", partial_count); }

    if let Some(options) = doc.get_options() {
        if !options.is_empty() {
            let mut sorted: Vec<_> = options.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            println!("  OPTIONS Overrides:");
            for (key, value) in sorted {
                println!("    - {}: {}", key, value);
            }
        }
    }
}

fn print_warnings(doc: &parser::GctfDocument) {
    print_warnings_for_doc(doc, 0);
}

fn print_warnings_for_doc(doc: &parser::GctfDocument, _doc_num: usize) {
    let sections = &doc.sections;
    let mut has_warnings = false;

    let has_endpoint = sections.iter().any(|s| s.section_type == SectionType::Endpoint);
    if !has_endpoint {
        println!("  [ERROR] No ENDPOINT section found");
        has_warnings = true;
    }

    let has_request = sections.iter().any(|s| s.section_type == SectionType::Request);
    let has_error = sections.iter().any(|s| s.section_type == SectionType::Error);
    if !has_request && !has_error {
        println!("  [WARN] No REQUEST or ERROR section found");
        has_warnings = true;
    }

    let has_response = sections.iter().any(|s| s.section_type == SectionType::Response);
    if has_request && !has_response && !has_error {
        println!("  [WARN] REQUEST section present but no RESPONSE or ERROR section");
        has_warnings = true;
    }

    // Semantic analysis
    let workflow = Workflow::from_document_with_analysis(doc);
    for event in workflow.semantic_analysis() {
        if let crate::execution::WorkflowEvent::SemanticAnalysis {
            type_mismatches, unknown_plugins,
        } = event {
            for mismatch in type_mismatches {
                println!("  [ERROR] Line {}: {}", mismatch.line, mismatch.message);
                println!("         Expression: {}", mismatch.expression.as_ref().unwrap_or(&"".to_string()));
                has_warnings = true;
            }
            for unknown in unknown_plugins {
                println!("  [ERROR] Line {}: {}", unknown.line, unknown.message);
                println!("         Assertion: {}", unknown.expression.as_ref().unwrap_or(&"".to_string()));
                has_warnings = true;
            }
        }
    }

    for hint in crate::optimizer::collect_assertion_optimizations(doc) {
        println!("  [HINT] Line {}: [{}] {} -> {}", hint.line, hint.rule_id, hint.before, hint.after);
        println!("         Boolean expression compared with true/false can be simplified");
        has_warnings = true;
    }

    for (i, section) in sections.iter().enumerate() {
        if section.inline_options.with_asserts {
            let has_following_asserts = sections[i + 1..]
                .iter()
                .take_while(|s| s.section_type != SectionType::Request)
                .any(|s| s.section_type == SectionType::Asserts);
            if !has_following_asserts {
                println!("  [WARN] Line {}: with_asserts option set but no", section.start_line + 1);
                println!("         ASSERTS section follows");
                has_warnings = true;
            }
        }

        if section.section_type == SectionType::Request
            && matches!(section.content, SectionContent::Empty)
        {
            println!("  [INFO] Line {}: Empty REQUEST section will send", section.start_line + 1);
            println!("         empty JSON object {{}}");
            has_warnings = true;
        }

        if section.section_type == SectionType::Extract
            && let SectionContent::Extract(extractions) = &section.content
            && extractions.is_empty()
        {
            println!("  [WARN] Line {}: EXTRACT section has no variables", section.start_line + 1);
            has_warnings = true;
        }

        if section.section_type == SectionType::Asserts
            && let SectionContent::Assertions(assertions) = &section.content
            && assertions.is_empty()
        {
            println!("  [WARN] Line {}: ASSERTS section has no assertions", section.start_line + 1);
            has_warnings = true;
        }
    }

    if !has_warnings {
        println!("  [OK] No structural warnings or hints.");
    }
}
