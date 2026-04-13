// Explain command - show detailed execution plan via Workflow

use crate::cli::args::HasFormat;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::cli::args::ExplainArgs;
use crate::execution::{ExecutionPlan, Workflow};
use crate::optimizer;
use crate::parser;
use crate::parser::ast::{SectionContent, SectionType};

fn sorted_key_values(map: &std::collections::HashMap<String, String>) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    pairs.sort_by(|(ka, _), (kb, _)| ka.cmp(kb));
    pairs
}

#[derive(Serialize)]
struct ExplainJsonOutput {
    semantic_plan: ExecutionPlan,
    optimization_trace: Vec<optimizer::OptimizationHint>,
    optimized_plan: ExecutionPlan,
    execution_plan: ExecutionPlan,
}

#[derive(Serialize)]
struct MultiDocExplainJson {
    documents: Vec<DocumentPlan>,
    optimization_trace: Vec<optimizer::OptimizationHint>,
}

#[derive(Serialize)]
struct DocumentPlan {
    index: usize,
    endpoint: Option<String>,
    address: Option<String>,
    execution_plan: ExecutionPlan,
    variable_extractions: Vec<(String, String)>,
    has_streaming: bool,
    rpc_mode: String,
}

pub async fn handle_explain(args: &ExplainArgs) -> Result<()> {
    let file_path = &args.file;
    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
    }

    let parse_start = std::time::Instant::now();

    let parse_result = parser::parse_with_recovery(file_path);
    let doc = parse_result.document;

    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    if !parse_result.diagnostics.is_empty() {
        eprintln!();
        eprintln!("PARSE DIAGNOSTICS");
        eprintln!("=================");
        eprintln!("File: {}", file_path.display());
        eprintln!("Recovered sections: {}", parse_result.recovered_sections);
        eprintln!("Failed sections: {}", parse_result.failed_sections);
        eprintln!();
        for d in &parse_result.diagnostics.diagnostics {
            crate::commands::print_diagnostic(d);
            eprintln!();
        }
    }

    if args.is_json() {
        // Backward compatible: single doc uses original format
        if doc.is_single_document() {
            let semantic_plan = ExecutionPlan::from_document(&doc);
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
            // Multi-doc: extended format
            let mut documents = Vec::new();
            let mut all_optimizations: Vec<optimizer::OptimizationHint> = Vec::new();

            for (doc_idx, d) in doc.iter_chain().enumerate() {
                let workflow = Workflow::from_document_with_analysis(d);
                let plan = ExecutionPlan::from_document(d);

                let mut extractions: Vec<(String, String)> = Vec::new();
                for section in &d.sections {
                    if section.section_type == SectionType::Extract
                        && let SectionContent::Extract(map) = &section.content
                    {
                        for (name, expr) in map {
                            extractions.push((name.clone(), expr.clone()));
                        }
                    }
                }
                extractions.sort_by(|a, b| a.0.cmp(&b.0));

                all_optimizations.extend(optimizer::collect_assertion_optimizations(d));

                documents.push(DocumentPlan {
                    index: doc_idx + 1,
                    endpoint: d.get_endpoint(),
                    address: d.get_address(None),
                    execution_plan: plan,
                    variable_extractions: extractions,
                    has_streaming: workflow.has_streaming(),
                    rpc_mode: workflow.rpc_mode_name().to_string(),
                });
            }

            let output = MultiDocExplainJson {
                documents,
                optimization_trace: all_optimizations,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else {
        // Text output
        let total_docs = doc.document_count();

        if total_docs > 1 {
            println!();
            println!("MULTI-DOCUMENT EXECUTION PLAN");
            println!("==============================");
            println!("File: {}", file_path.display());
            println!("Documents: {}", total_docs);
            println!();
        } else {
            println!();
            println!("EXECUTION PLAN");
            println!("==============");
            println!();
        }

        // Print META once at file level (before documents)
        if total_docs > 1 {
            for section in &doc.sections {
                if section.section_type == SectionType::Meta {
                    if let SectionContent::Meta(m) = &section.content {
                        println!("META");
                        println!("----");
                        if !m.tags.is_empty() {
                            println!("  tags: {:?}", m.tags);
                        }
                        println!();
                        break;
                    }
                }
            }
        }

        // Print each document via workflow
        for (doc_idx, d) in doc.iter_chain().enumerate() {
            if total_docs > 1 {
                print_doc_scenario(doc_idx + 1, d);
            } else {
                print_single_doc_workflow(d, file_path);
            }
        }

        // Collect all optimizer hints
        let mut all_optimizations: Vec<optimizer::OptimizationHint> = Vec::new();
        for d in doc.iter_chain() {
            all_optimizations.extend(optimizer::collect_assertion_optimizations(d));
        }

        println!("OPTIMIZATION TRACE:");
        if all_optimizations.is_empty() {
            println!("  (no safe rewrites found)");
        } else {
            for hint in &all_optimizations {
                println!(
                    "  - [{}] line {}: {} -> {}",
                    hint.rule_id, hint.line, hint.before, hint.after
                );
            }
        }
        println!();

        // Validation summary
        println!("VALIDATION:");
        let mut all_valid = true;
        for d in doc.iter_chain() {
            if let Err(e) = parser::validate_document(d) {
                println!("  FAILED: {}", e);
                all_valid = false;
            }
        }
        if all_valid {
            println!("  OK - All documents structurally valid.");
        }
        println!();

        println!("TIMING:");
        println!("  Parse:      {:.3}ms", parse_ms);
    }

    Ok(())
}

fn print_doc_scenario(doc_idx: usize, doc: &parser::GctfDocument) {
    let endpoint = doc.get_endpoint().unwrap_or_else(|| "unknown".to_string());
    println!("SCENARIO {}: {}", doc_idx, endpoint);
    println!("  {}", "-".repeat(60));

    // Connection
    if let Some(addr) = doc.get_address(None) {
        println!("  → Connect: {}", addr);
    }

    // Request headers
    if let Some(headers) = doc.get_request_headers() {
        println!("  → Request headers:");
        for (key, value) in sorted_key_values(&headers) {
            println!("    {}: {}", key, value);
        }
    }

    // Options
    if let Some(options) = doc.get_options()
        && !options.is_empty()
    {
        println!("  → Options:");
        for (key, value) in sorted_key_values(&options) {
            println!("    {}: {}", key, value);
        }
    }

    // TLS
    if let Some(tls) = doc.get_tls_config()
        && !tls.is_empty()
    {
        println!("  → TLS config:");
        for (key, value) in sorted_key_values(&tls) {
            println!("    {}: {}", key, value);
        }
    }

    // Proto
    if let Some(proto) = doc.get_proto_config()
        && !proto.is_empty()
    {
        println!("  → Proto config:");
        for (key, value) in sorted_key_values(&proto) {
            println!("    {}: {}", key, value);
        }
    }

    // Requests
    let requests = doc.get_requests();
    if !requests.is_empty() {
        if requests.len() == 1 {
            let json_str = serde_json::to_string_pretty(&requests[0])
                .unwrap_or_else(|_| requests[0].to_string());
            println!("  → Send:");
            for line in json_str.lines() {
                println!("    {}", line);
            }
        } else {
            println!("  → Send {} request(s) (client streaming):", requests.len());
            for (i, req) in requests.iter().enumerate() {
                let json_str =
                    serde_json::to_string_pretty(req).unwrap_or_else(|_| req.to_string());
                println!("    Request #{}:", i + 1);
                for line in json_str.lines() {
                    println!("      {}", line);
                }
            }
        }
    }

    // Expected response
    for section in &doc.sections {
        match section.section_type {
            SectionType::Response => {
                match &section.content {
                    SectionContent::Json(value) => {
                        let json_str = serde_json::to_string_pretty(value)
                            .unwrap_or_else(|_| value.to_string());
                        println!("  ← Expect response:");
                        for line in json_str.lines() {
                            println!("    {}", line);
                        }
                    }
                    SectionContent::JsonLines(values) => {
                        println!(
                            "  ← Expect {} response(s) (server streaming):",
                            values.len()
                        );
                        for (i, v) in values.iter().enumerate() {
                            let json_str =
                                serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
                            println!("    Response #{}:", i + 1);
                            for line in json_str.lines() {
                                println!("      {}", line);
                            }
                        }
                    }
                    _ => {}
                }
                if section.inline_options.with_asserts {
                    println!("    [with_asserts]");
                }
                if section.inline_options.partial {
                    println!("    [partial]");
                }
            }
            SectionType::Error => {
                if let SectionContent::Json(value) = &section.content {
                    let json_str =
                        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
                    println!("  ← Expect error:");
                    for line in json_str.lines() {
                        println!("    {}", line);
                    }
                }
            }
            _ => {}
        }
    }

    // Extract
    let extractions: Vec<_> = doc
        .sections
        .iter()
        .filter(|s| s.section_type == SectionType::Extract)
        .flat_map(|s| match &s.content {
            SectionContent::Extract(map) => map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();

    if !extractions.is_empty() {
        println!("  ↓ Extract:");
        for (name, expr) in &extractions {
            println!("    {} = {}", name, expr);
        }
    }

    // Assertions from workflow
    let workflow = Workflow::from_document_with_analysis(doc);
    for event in &workflow.events {
        if let crate::execution::WorkflowEvent::Assert {
            count, line_range, ..
        } = event
        {
            println!(
                "  ✓ {} assertion(s) at lines {}-{}",
                count,
                line_range.0 + 1,
                line_range.1 + 1
            );
        }
        if let crate::execution::WorkflowEvent::SemanticAnalysis {
            type_mismatches,
            unknown_plugins,
        } = event
            && (!type_mismatches.is_empty() || !unknown_plugins.is_empty())
        {
            println!("  ⚠ Semantic issues:");
            for m in type_mismatches {
                println!("    - {}", m.message);
            }
            for u in unknown_plugins {
                println!("    - {}", u.message);
            }
        }
    }

    // Print actual assertion expressions
    for section in &doc.sections {
        if section.section_type == SectionType::Asserts {
            if let SectionContent::Assertions(assertions) = &section.content {
                for (i, a) in assertions.iter().enumerate() {
                    let rewritten = optimizer::rewrite_assertion_expression_fixed_point(a);
                    println!("    {}. {}", i + 1, rewritten);
                }
            }
        }
    }

    println!();
}

fn print_single_doc_workflow(doc: &parser::GctfDocument, file_path: &Path) {
    println!("FILE: {}", file_path.display());
    println!();

    // META section (if present)
    for section in &doc.sections {
        if section.section_type == SectionType::Meta {
            if let SectionContent::Meta(m) = &section.content {
                println!("META");
                println!("----");
                if let Some(name) = &m.name {
                    println!("  name: {}", name);
                }
                if let Some(summary) = &m.summary {
                    println!("  summary: {}", summary);
                }
                if !m.tags.is_empty() {
                    println!("  tags: {:?}", m.tags);
                }
                if let Some(owner) = &m.owner {
                    println!("  owner: {}", owner);
                }
                if !m.links.is_empty() {
                    println!("  links: {:?}", m.links);
                }
                println!();
                break;
            }
        }
    }

    // Connection info
    println!("CONNECTION");
    println!("----------");
    if let Some(addr) = doc.get_address(None) {
        println!("  Address: {}", addr);
    } else {
        println!("  Address: (from GRPCTESTIFY_ADDRESS env or default)");
    }
    println!();

    // Endpoint
    println!("TARGET ENDPOINT");
    println!("---------------");
    if let Some(endpoint) = doc.get_endpoint() {
        println!("  Endpoint: {}", endpoint);
        if let Some((pkg, svc, method)) = doc.parse_endpoint() {
            if !pkg.is_empty() {
                println!("  Package: {}", pkg);
            }
            println!("  Service: {}", svc);
            println!("  Method:  {}", method);
        }
    }
    println!();

    // Workflow
    let workflow = Workflow::from_document_with_analysis(doc);

    println!("EXECUTION WORKFLOW");
    println!("------------------");

    // Headers
    if let Some(headers) = doc.get_request_headers() {
        println!();
        println!("REQUEST HEADERS");
        for (key, value) in sorted_key_values(&headers) {
            println!("  {}: {}", key, value);
        }
    }

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
                    for (key, value) in sorted_key_values(headers) {
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
                        println!("  {{}} (empty request)");
                    }
                    _ => {}
                }
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
                        let json_str = serde_json::to_string_pretty(value)
                            .unwrap_or_else(|_| value.to_string());
                        println!("  Expected:");
                        for line in json_str.lines() {
                            println!("    {}", line);
                        }
                    }
                    SectionContent::JsonLines(values) => {
                        println!("  Expected {} response(s):", values.len());
                        for (i, v) in values.iter().enumerate() {
                            let json_str =
                                serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
                            println!("    {}. {}", i + 1, json_str);
                        }
                    }
                    _ => {}
                }
                let mut opts = Vec::new();
                if section.inline_options.with_asserts {
                    opts.push("with_asserts");
                }
                if section.inline_options.partial {
                    opts.push("partial");
                }
                if !opts.is_empty() {
                    println!("  Options: {}", opts.join(", "));
                }
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
                    let json_str =
                        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
                    println!("  Expected error:");
                    for line in json_str.lines() {
                        println!("    {}", line);
                    }
                }
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
                    for (name, expr) in sorted_key_values(extractions) {
                        println!("  ${{ {} }} = {}", name, expr);
                    }
                }
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
                    for (i, a) in assertions.iter().enumerate() {
                        let rewritten = optimizer::rewrite_assertion_expression_fixed_point(a);
                        println!("  {}. {}", i + 1, rewritten);
                    }
                }
                step += 1;
            }
            SectionType::Options => {
                println!();
                println!(
                    "Step {}: OPTIONS [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::KeyValues(options) = &section.content {
                    if options.is_empty() {
                        println!("  (no runtime overrides)");
                    } else {
                        println!("  Runtime overrides:");
                        for (key, value) in sorted_key_values(options) {
                            println!("    {}: {}", key, value);
                        }
                    }
                }
                step += 1;
            }
            SectionType::Tls | SectionType::Proto => {
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
            SectionType::Meta => {}
        }
    }

    // Summary
    println!();
    println!("EXECUTION SUMMARY");
    println!("-----------------");
    println!("  RPC Mode: {}", workflow.rpc_mode_name());
    if workflow.has_streaming() {
        println!("  Streaming: enabled");
    }
}
