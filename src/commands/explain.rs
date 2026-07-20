// Explain command - show detailed execution plan via Workflow

use crate::bench::sources::index_builder::index_path_for_source;
use crate::cli::args::HasFormat;
use crate::utils::file::FileUtils;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::bench::schema::bench_key_rank;
use crate::cli::args::ExplainArgs;
use crate::commands::bench::{BenchConfigResolved, BenchOptionSource};
use crate::execution::runner_helpers::{CliRuntimeDefaults, resolve_effective_runtime_options};
use crate::execution::{ExecutionPlan, Workflow};
use crate::optimizer;
use crate::parser;
use crate::parser::ast::{GctfDocument, SectionContent, SectionType};

fn optimization_hints_from_workflow(workflow: &Workflow) -> Vec<optimizer::OptimizationHint> {
    let mut hints = Vec::new();
    for event in workflow.optimization_hints() {
        if let crate::execution::WorkflowEvent::OptimizationFound { hints: ev_hints } = event {
            for hint in ev_hints {
                if let Ok(rule_id) = optimizer::RuleId::try_from(hint.rule_id.as_str()) {
                    hints.push(optimizer::OptimizationHint {
                        rule_id,
                        line: hint.line,
                        before: hint.before.clone(),
                        after: hint.after.clone(),
                        preconditions: None,
                        negative_cases: None,
                        proof_note: None,
                    });
                }
            }
        }
    }
    hints
}

fn validation_passed_from_workflow(workflow: &Workflow) -> bool {
    for event in workflow.validation_results() {
        if let crate::execution::WorkflowEvent::ValidationResult { passed, .. } = event {
            return *passed;
        }
    }
    true
}

fn sorted_key_values(map: &std::collections::HashMap<String, String>) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    pairs.sort_by_key(|(ka, _)| *ka);
    pairs
}

fn sorted_bench_key_values(map: &std::collections::HashMap<String, String>) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = map.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    pairs.sort_by(|(ka, _), (kb, _)| {
        bench_key_rank(ka)
            .cmp(&bench_key_rank(kb))
            .then_with(|| ka.cmp(kb))
    });
    pairs
}

#[derive(Serialize)]
struct ExplainJsonOutput {
    plan: ExecutionPlan,
    optimization_trace: Vec<optimizer::OptimizationHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bench_resolved: Option<Vec<crate::report::BenchResolvedOption>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    bench_resolved: Option<Vec<crate::report::BenchResolvedOption>>,
}

fn bench_section_map(
    doc: &parser::GctfDocument,
) -> Option<&std::collections::HashMap<String, String>> {
    doc.sections.iter().find_map(|section| {
        if section.section_type == SectionType::Bench
            && let SectionContent::KeyValues(bench) = &section.content
        {
            return Some(bench);
        }
        None
    })
}

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
            .unwrap_or(BenchOptionSource::Default)
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
            let workflow = Workflow::from_document_with_analysis(&doc);
            let plan = ExecutionPlan::from_document(&doc);
            let optimization_trace = optimization_hints_from_workflow(&workflow);
            let output = ExplainJsonOutput {
                plan,
                optimization_trace,
                bench_resolved: bench_resolved_options(&doc),
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

                all_optimizations.extend(optimization_hints_from_workflow(&workflow));

                documents.push(DocumentPlan {
                    index: doc_idx + 1,
                    endpoint: d.get_endpoint(),
                    address: d.get_address(None),
                    execution_plan: plan,
                    variable_extractions: extractions,
                    has_streaming: workflow.has_streaming(),
                    rpc_mode: workflow.rpc_mode_name().to_string(),
                    bench_resolved: bench_resolved_options(d),
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
                if section.section_type == SectionType::Meta
                    && let SectionContent::Meta(m) = &section.content
                {
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
            let workflow = Workflow::from_document_with_analysis(d);
            all_optimizations.extend(optimization_hints_from_workflow(&workflow));
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
            let workflow = Workflow::from_document_with_analysis(d);
            if !validation_passed_from_workflow(&workflow) {
                if let Some(crate::execution::WorkflowEvent::ValidationResult { errors, .. }) =
                    workflow.validation_results().first().copied()
                {
                    for e in errors {
                        println!("  FAILED: {}", e);
                    }
                }
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

    if let Some(addr) = doc.get_address(None) {
        println!("  → Connect: {}", addr);
    }

    if let Some(headers) = doc.get_request_headers() {
        println!("  → Request headers:");
        for (key, value) in sorted_key_values(&headers) {
            println!("    {}: {}", key, value);
        }
    }

    if let Some(options) = doc.get_options()
        && !options.is_empty()
    {
        println!("  → Options:");
        for (key, value) in sorted_key_values(&options) {
            println!("    {}: {}", key, value);
        }
    }

    if let Ok(effective) = resolve_effective_runtime_options(
        doc,
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    ) {
        println!("  -> Effective runtime:");
        println!(
            "    timeout={}s ({:?}), retry={} ({:?}), retry_delay={}s ({:?}), no_retry={} ({:?}), compression={} ({:?})",
            effective.timeout_seconds.value,
            effective.timeout_seconds.source,
            effective.retry.value,
            effective.retry.source,
            effective.retry_delay_seconds.value,
            effective.retry_delay_seconds.source,
            effective.no_retry.value,
            effective.no_retry.source,
            effective.compression.value,
            effective.compression.source,
        );
    }

    if let Some(tls) = doc.get_tls_config()
        && !tls.is_empty()
    {
        println!("  → TLS config:");
        for (key, value) in sorted_key_values(&tls) {
            println!("    {}: {}", key, value);
        }
    }

    if let Some(proto) = doc.get_proto_config()
        && !proto.is_empty()
    {
        println!("  → Proto config:");
        for (key, value) in sorted_key_values(&proto) {
            println!("    {}: {}", key, value);
        }
    }

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
                if section.inline_options.with_asserts {
                    println!("    [with_asserts]");
                }
                if section.inline_options.partial {
                    println!("    [partial]");
                }
            }
            _ => {}
        }
    }

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

    for section in &doc.sections {
        if section.section_type == SectionType::Asserts
            && let SectionContent::Assertions(assertions) = &section.content
        {
            for (i, a) in assertions.iter().enumerate() {
                let rewritten = optimizer::rewrite_assertion_expression_fixed_point(a);
                println!("    {}. {}", i + 1, rewritten);
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
        if section.section_type == SectionType::Meta
            && let SectionContent::Meta(m) = &section.content
        {
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

    // ATTRIBUTES section (if any)
    let mut has_attrs = false;
    for section in &doc.sections {
        if !section.attributes.is_empty() {
            if !has_attrs {
                println!("ATTRIBUTES");
                println!("----------");
                has_attrs = true;
            }
            print!("  [{}] ", section.section_type.as_str());
            let mut first = true;
            for attr in &section.attributes {
                if !first {
                    print!(" ");
                }
                first = false;
                print!("{}", attr.format_directive());
            }
            println!();
        }
    }
    if has_attrs {
        println!();
    }

    println!("CONNECTION");
    println!("----------");
    if let Some(addr) = doc.get_address(None) {
        println!("  Address: {}", addr);
    } else {
        println!("  Address: (from GRPCTESTIFY_ADDRESS env or default)");
    }
    if let Ok(effective) = resolve_effective_runtime_options(
        doc,
        CliRuntimeDefaults {
            timeout_seconds: 30,
            retry: 0,
            retry_delay_seconds: 1.0,
            no_retry: false,
        },
    ) {
        println!(
            "  Runtime: timeout={}s ({:?}), retry={} ({:?}), retry_delay={}s ({:?}), no_retry={} ({:?}), compression={} ({:?})",
            effective.timeout_seconds.value,
            effective.timeout_seconds.source,
            effective.retry.value,
            effective.retry.source,
            effective.retry_delay_seconds.value,
            effective.retry_delay_seconds.source,
            effective.no_retry.value,
            effective.no_retry.source,
            effective.compression.value,
            effective.compression.source,
        );
    }
    println!();

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

    let workflow = Workflow::from_document_with_analysis(doc);

    println!("EXECUTION WORKFLOW");
    println!("------------------");

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
                } else if matches!(section.content, SectionContent::Empty) {
                    println!("  Expected error (no body, assertions-only)");
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
            SectionType::Bench => {
                println!();
                println!(
                    "Step {}: BENCH [lines {}-{}]",
                    step,
                    section.start_line + 1,
                    section.end_line + 1
                );
                if let SectionContent::KeyValues(bench) = &section.content {
                    if bench.is_empty() {
                        println!("  (no benchmark options)");
                    } else {
                        println!("  Benchmark profile:");
                        for (key, value) in sorted_bench_key_values(bench) {
                            println!("    {}: {}", key, value);
                        }

                        match BenchConfigResolved::from_bench_section(Some(bench)) {
                            Ok(config) => {
                                println!("  Resolved schedule/runtime (value + source):");
                                for key in [
                                    "concurrency",
                                    "load_schedule",
                                    "load_start",
                                    "load_step",
                                    "load_end",
                                    "load_step_duration",
                                    "load_max_duration",
                                    "progress_interval",
                                ] {
                                    let source = config
                                        .option_sources
                                        .get(key)
                                        .copied()
                                        .unwrap_or(BenchOptionSource::Default)
                                        .as_str();
                                    let value = match key {
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
                                        "progress_interval" => {
                                            format!("{}ms", config.progress_interval.as_millis())
                                        }
                                        _ => "<n/a>".to_string(),
                                    };
                                    println!("    {}: {} (source: {})", key, value, source);
                                }
                            }
                            Err(err) => {
                                println!("  [warn] unresolved BENCH config: {}", err);
                            }
                        }
                    }
                }
                step += 1;
            }
        }
    }

    println!();
    println!("EXECUTION SUMMARY");
    println!("-----------------");
    println!("  RPC Mode: {}", workflow.rpc_mode_name());
    if workflow.has_streaming() {
        println!("  Streaming: enabled");
    }

    print_source_hints(doc, file_path);
    print_type_optimization_hints(doc, file_path);
}

fn print_type_optimization_hints(doc: &GctfDocument, file_path: &Path) {
    use crate::bench::sources::SourceDefinition;
    use crate::bench::sources::index::infer_key_type_from_stream;

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

    let mut hints_printed = false;

    for def in &sources {
        let source_path = FileUtils::resolve_relative_path(file_path, &def.file);
        if !source_path.exists() {
            continue;
        }

        for key_col in def.indexed_columns() {
            let file = match std::fs::File::open(&source_path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut reader = std::io::BufReader::new(file);

            let key_column_idx =
                match crate::bench::sources::index_builder::find_column_index(&mut reader, key_col)
                {
                    Ok(idx) => idx,
                    Err(_) => continue,
                };

            let file_size = source_path.metadata().map(|m| m.len()).unwrap_or(0);
            let max_bytes_scan = file_size.min(1024 * 1024);

            let (inferred_type, stats) =
                match infer_key_type_from_stream(&mut reader, key_column_idx, 1000, max_bytes_scan)
                {
                    Ok((t, s)) => (t, s),
                    Err(_) => continue,
                };

            let suggested_type = match inferred_type {
                crate::bench::sources::index::KeyType::String => None,
                crate::bench::sources::index::KeyType::U64 => Some("u64"),
                crate::bench::sources::index::KeyType::I64 => Some("i64"),
                crate::bench::sources::index::KeyType::U32 => Some("u32"),
                crate::bench::sources::index::KeyType::I32 => Some("i32"),
                crate::bench::sources::index::KeyType::UnixTimestampSec => Some("timestamp"),
                crate::bench::sources::index::KeyType::UnixTimestampMillis => Some("timestamp_ms"),
                crate::bench::sources::index::KeyType::DatePacked => Some("date"),
                crate::bench::sources::index::KeyType::TimePacked => Some("time"),
                crate::bench::sources::index::KeyType::UUID => Some("uuid"),
                crate::bench::sources::index::KeyType::ULID => Some("ulid"),
            };

            if let Some(suggested) = suggested_type {
                if !hints_printed {
                    println!();
                    println!("TYPE OPTIMIZATION HINTS");
                    println!("------------------------");
                    hints_printed = true;
                }

                let name_display =
                    format!("{}.{}", def.name.as_deref().unwrap_or(&def.file), key_col);
                println!(
                    "  {} {name_display}: consider `indexed_by: {}: {}` (inferred from {} samples, {}% confidence)",
                    if stats.confidence >= 0.9 {
                        "✓"
                    } else {
                        "⚠"
                    },
                    key_col,
                    suggested,
                    stats.samples_taken,
                    (stats.confidence * 100.0) as i32
                );
            }
        }
    }

    if hints_printed {
        println!();
        println!("  Explicit type annotations improve lookup performance for non-String keys.");
    }
}

fn print_source_hints(doc: &GctfDocument, file_path: &Path) {
    use crate::bench::sources::SourceDefinition;
    use crate::bench::sources::analyzer::SourceUsageAnalyzer;

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

    if plan.required_indexes.is_empty() {
        return;
    }

    let mut hints_printed = false;
    for req in &plan.required_indexes {
        let source_def = sources.iter().find(|s| {
            let from_name = s.name.as_deref() == Some(&req.source);
            let from_file_stem = std::path::Path::new(&s.file)
                .file_stem()
                .map(|stem| stem.to_string_lossy())
                .is_some_and(|stem| stem.as_ref() == req.source);
            from_name || from_file_stem
        });

        let (source_file_path, key_columns): (&str, Vec<&str>) = match source_def {
            Some(s) => (s.file.as_str(), s.indexed_columns()),
            None => (&req.source, vec![]),
        };

        let resolved_path = FileUtils::resolve_relative_path(file_path, source_file_path);
        let key_column = key_columns.first().copied().unwrap_or(req.column.as_str());
        let idx_path = index_path_for_source(&resolved_path, key_column);

        let idx_exists = idx_path.exists();
        let (idx_fresh, idx_corrupted) = if idx_exists {
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
            let corrupted = !crate::bench::sources::index::is_index_valid(&idx_path);
            (fresh, corrupted)
        } else {
            (false, false)
        };

        if !hints_printed {
            println!();
            println!("SOURCE INDEX ANALYSIS");
            println!("---------------------");
            hints_printed = true;
        }

        let status = if idx_corrupted {
            "✗ corrupted"
        } else if idx_fresh {
            "✓ fresh"
        } else if idx_exists {
            "⚠ stale"
        } else {
            "✗ missing"
        };

        let cmd_hint = if idx_corrupted {
            " (run `grpctestify index --force` to rebuild)"
        } else {
            ""
        };

        println!("  {} {}.{}{}", status, req.source, req.column, cmd_hint);
    }

    if hints_printed {
        println!();
        println!(
            "  → Run `grpctestify index {}` to build/update indexes",
            file_path.display()
        );
    }
}
