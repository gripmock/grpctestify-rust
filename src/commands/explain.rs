// Explain command - show detailed execution plan and workflow

use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::cli::args::ExplainArgs;
use crate::execution;
use crate::optimizer;
use crate::parser;
use crate::parser::ast::{SectionContent, SectionType};

#[derive(Serialize)]
struct ExplainJsonOutput {
    semantic_plan: execution::ExecutionPlan,
    optimization_trace: Vec<optimizer::OptimizationHint>,
    optimized_plan: execution::ExecutionPlan,
    execution_plan: execution::ExecutionPlan,
}

pub async fn handle_explain(args: &ExplainArgs) -> Result<()> {
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
    let semantic_type_mismatches = crate::semantics::collect_assertion_type_mismatches(&doc);
    let semantic_unknown_plugins = crate::semantics::collect_unknown_plugin_calls(&doc);

    if args.is_json() {
        let semantic_plan = execution::ExecutionPlan::from_document(&doc);
        let optimization_trace = optimizer::collect_assertion_optimizations(&doc);
        let optimized_plan = semantic_plan.clone();
        let execution_plan = optimized_plan.clone();
        let output = ExplainJsonOutput {
            semantic_plan,
            optimization_trace,
            optimized_plan,
            execution_plan,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Print detailed workflow visualization
        print_detailed_workflow(&doc, file_path, &parse_diagnostics);
        println!();

        let optimization_trace = optimizer::collect_assertion_optimizations(&doc);
        println!("OPTIMIZATION TRACE:");
        if optimization_trace.is_empty() {
            println!("  (no safe rewrites found)");
        } else {
            for hint in &optimization_trace {
                println!(
                    "  - [{}] line {}: {} -> {}",
                    hint.rule_id, hint.line, hint.before, hint.after
                );
            }
        }
        println!();

        // Show validation result
        println!("VALIDATION:");
        match validation_result {
            Err(e) => println!("  FAILED: {}", e),
            Ok(_)
                if !semantic_unknown_plugins.is_empty() || !semantic_type_mismatches.is_empty() =>
            {
                println!(
                    "  FAILED: semantic validation failed ({} unknown plugin call(s), {} type mismatch(es))",
                    semantic_unknown_plugins.len(),
                    semantic_type_mismatches.len()
                );
                for unknown in &semantic_unknown_plugins {
                    println!(
                        "  - line {} [{}] {}",
                        unknown.line, unknown.rule_id, unknown.message
                    );
                }
                for mismatch in &semantic_type_mismatches {
                    println!(
                        "  - line {} [{}] {}",
                        mismatch.line, mismatch.rule_id, mismatch.message
                    );
                }
            }
            Ok(_) => println!("  OK - No issues found. Test appears structurally valid."),
        }
        println!();
        println!("TIMING:");
        println!("  Parse:      {:.3}ms", parse_ms);
        println!("  Validation: {:.3}ms", validation_ms);
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

fn print_detailed_workflow(
    doc: &parser::GctfDocument,
    file_path: &Path,
    parse_diagnostics: &parser::ParseDiagnostics,
) {
    println!();
    println!("EXECUTION PLAN");
    println!("==============");
    println!();
    println!("FILE: {}", file_path.display());
    println!();

    println!("PARSE PROFILING");
    println!("---------------");
    println!("  File size:         {} bytes", parse_diagnostics.bytes);
    println!("  Total lines:       {}", parse_diagnostics.total_lines);
    println!("  Section headers:   {}", parse_diagnostics.section_headers);
    println!(
        "  Read:              {:.3}ms",
        parse_diagnostics.timings.read_ms
    );
    println!(
        "  Parse sections:    {:.3}ms",
        parse_diagnostics.timings.parse_sections_ms
    );
    println!(
        "  Build document:    {:.3}ms",
        parse_diagnostics.timings.build_document_ms
    );
    println!(
        "  Total:             {:.3}ms",
        parse_diagnostics.timings.total_ms
    );
    println!();

    // Build execution plan for summary
    let plan = execution::ExecutionPlan::from_document(doc);

    // Connection info
    println!("CONNECTION");
    println!("----------");
    println!("  Address: {}", plan.connection.address);
    println!("  Source:  {}", plan.connection.source);
    println!();

    // Target endpoint
    println!("TARGET ENDPOINT");
    println!("---------------");
    println!("  Endpoint: {}", plan.target.endpoint);
    if let Some(pkg) = &plan.target.package {
        println!("  Package: {}", pkg);
    }
    if let Some(svc) = &plan.target.service {
        println!("  Service: {}", svc);
    }
    if let Some(method) = &plan.target.method {
        println!("  Method:  {}", method);
    }
    println!("  RPC Mode: {}", plan.summary.rpc_mode_name);
    println!();

    // Request headers
    if let Some(headers) = &plan.headers {
        println!("REQUEST HEADERS ({})", headers.count);
        println!("-----------------");
        for (key, value) in &headers.headers {
            println!("  {}: {}", key, value);
        }
        println!();
    }

    // Detailed workflow by sections
    println!("EXECUTION WORKFLOW");
    println!("------------------");

    let mut step = 1;
    for section in &doc.sections {
        match section.section_type {
            SectionType::Address => {
                if let SectionContent::Single(addr) = &section.content {
                    println!();
                    println!(
                        "Step {}: ADDRESS [lines {}-{}]",
                        step,
                        section.start_line + 1,
                        section.end_line + 1
                    );
                    println!("  {}", addr);
                    step += 1;
                }
            }
            SectionType::Endpoint => {
                if let SectionContent::Single(endpoint) = &section.content {
                    println!();
                    println!(
                        "Step {}: ENDPOINT [lines {}-{}]",
                        step,
                        section.start_line + 1,
                        section.end_line + 1
                    );
                    println!("  {}", endpoint);
                    step += 1;
                }
            }
            SectionType::RequestHeaders => {
                println!();
                println!(
                    "Step {}: REQUEST HEADERS [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::KeyValues(headers) = &section.content {
                    for (key, value) in headers {
                        println!("  {}: {}", key, value);
                    }
                }
                step += 1;
            }
            SectionType::Request => {
                println!();
                println!(
                    "Step {}: REQUEST [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                match &section.content {
                    SectionContent::Json(value) => {
                        let json_str = serde_json::to_string_pretty(value)
                            .unwrap_or_else(|_| value.to_string());
                        for line in json_str.lines() {
                            println!("  {}", line);
                        }
                    }
                    SectionContent::Empty => {
                        println!("  {{}} (empty request - will send empty JSON object)");
                    }
                    _ => {}
                }
                println!("  Action: Send request to gRPC server");
                step += 1;
            }
            SectionType::Response => {
                println!();
                println!(
                    "Step {}: RESPONSE [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                match &section.content {
                    SectionContent::Json(value) => {
                        println!("  Expected response:");
                        let json_str = serde_json::to_string_pretty(value)
                            .unwrap_or_else(|_| value.to_string());
                        for line in json_str.lines() {
                            println!("  {}", line);
                        }
                    }
                    SectionContent::JsonLines(lines) => {
                        println!("  Expected {} response message(s):", lines.len());
                        for (idx, line) in lines.iter().enumerate() {
                            println!("    {}. {}", idx + 1, line);
                        }
                    }
                    _ => {}
                }

                // Show comparison options
                let mut option_flags = Vec::new();
                if section.inline_options.partial {
                    option_flags.push("partial");
                }
                if section.inline_options.unordered_arrays {
                    option_flags.push("unordered_arrays");
                }
                if section.inline_options.with_asserts {
                    option_flags.push("with_asserts");
                }
                if !option_flags.is_empty() {
                    println!("  Options: {}", option_flags.join(", "));
                }
                if !section.inline_options.redact.is_empty() {
                    println!("  Redact:  {:?}", section.inline_options.redact);
                }
                if let Some(tol) = section.inline_options.tolerance {
                    println!("  Tolerance: {}", tol);
                }
                println!("  Action: Validate response against expected");
                step += 1;
            }
            SectionType::Error => {
                println!();
                println!(
                    "Step {}: EXPECTED ERROR [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::Json(value) = &section.content {
                    println!("  Expected gRPC error:");
                    if let Some(code) = value.get("code") {
                        println!("    Code: {}", code);
                    }
                    if let Some(message) = value.get("message") {
                        println!("    Message: {}", message);
                    }
                }
                println!("  Action: Verify gRPC error status and message");
                step += 1;
            }
            SectionType::Extract => {
                println!();
                println!(
                    "Step {}: EXTRACT [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::Extract(extractions) = &section.content {
                    for (var_name, jq_path) in extractions {
                        println!("  ${{ {} }} = {}", var_name, jq_path);
                    }
                }
                println!("  Action: Store variables for use in subsequent requests/assertions");
                step += 1;
            }
            SectionType::Asserts => {
                println!();
                println!(
                    "Step {}: ASSERTS [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::Assertions(assertions) = &section.content {
                    for (idx, assertion) in assertions.iter().enumerate() {
                        let rewritten =
                            optimizer::rewrite_assertion_expression_fixed_point(assertion);
                        if rewritten == assertion.trim() {
                            println!("  {}. {}", idx + 1, assertion);
                        } else {
                            println!("  {}. {}", idx + 1, rewritten);
                        }
                    }
                }
                println!("  Action: Evaluate optimized assertions (must all pass)");
                step += 1;
            }
            SectionType::Options | SectionType::Tls | SectionType::Proto => {
                println!();
                println!(
                    "Step {}: {} [lines {}-{}]",
                    step,
                    section.section_type.as_str(),
                    section.start_line + 1,
                    section.end_line + 1
                );
                println!("  (configuration section)");
                step += 1;
            }
        }
    }

    // Summary
    println!();
    println!("EXECUTION SUMMARY");
    println!("-----------------");
    println!("  Total Requests:       {}", plan.summary.total_requests);
    println!("  Total Responses:      {}", plan.summary.total_responses);
    if plan.summary.total_errors > 0 {
        println!("  Total Errors:         {}", plan.summary.total_errors);
    }
    println!(
        "  Variable Extractions: {}",
        plan.summary.variable_extractions
    );
    println!("  Assertion Blocks:     {}", plan.summary.assertion_blocks);
    println!("  RPC Mode:             {}", plan.summary.rpc_mode_name);

    // Workflow type description
    println!();
    println!("WORKFLOW TYPE");
    println!("-------------");
    print_workflow_type(doc);
}

fn print_workflow_type(doc: &parser::GctfDocument) {
    use crate::execution::{build_workflow_graph, get_call_type, get_workflow_summary};

    let summary = get_workflow_summary(&doc.sections);
    let call_type = get_call_type(&summary);

    println!("  {}", call_type);
    println!(
        "  Steps: {}",
        summary.total_requests
            + summary.total_responses
            + summary.total_errors
            + summary.total_extractions
            + summary.total_assertions
            + 1
    );
    println!("  Flow:");

    // Build and print workflow steps with section references
    let steps = build_workflow_graph(&doc.sections);
    for step in &steps {
        println!("    {} [line {}]", step.format(), step.section_line + 1);
    }
}
