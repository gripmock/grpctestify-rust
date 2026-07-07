// Check command - validate GCTF files

use crate::cli::args::HasFormat;
use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::bench::schema::bench_key_rank;
use crate::cli::args::CheckArgs;
use crate::parser;
use crate::parser::ErrorSeverity;
use crate::parser::ast::SectionType;
use crate::report::{CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity};
use crate::semantics;
use crate::utils::FileUtils;

fn sort_and_dedup_files(files: &mut Vec<PathBuf>) {
    files.sort();
    files.dedup();
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        (
            a.file.as_str(),
            a.range.start.line,
            a.code.as_str(),
            a.message.as_str(),
        )
            .cmp(&(
                b.file.as_str(),
                b.range.start.line,
                b.code.as_str(),
                b.message.as_str(),
            ))
    });
}

fn build_summary(
    diagnostics: &[Diagnostic],
    total_files: usize,
    files_with_errors: usize,
) -> CheckSummary {
    let total_errors = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
        .count();
    let total_warnings = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, DiagnosticSeverity::Warning))
        .count();
    CheckSummary {
        total_files,
        files_with_errors,
        total_errors,
        total_warnings,
    }
}

fn print_text_diagnostics(diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        println!(
            "{}:{}: [{}] {}",
            d.file, d.range.start.line, d.code, d.message
        );
        if let Some(hint) = &d.hint {
            println!("  hint: {}", hint);
        }
    }
}

fn validation_hint(message: &str) -> Option<&'static str> {
    if message.contains("OPTIONS.retry_delay is deprecated") {
        return Some("Use `retry_delay` in OPTIONS (snake_case canonical form)");
    }
    if message.contains("OPTIONS.no_retry is deprecated") {
        return Some("Use `no_retry` in OPTIONS (snake_case canonical form)");
    }
    if message.contains("OPTIONS.no_retry=true conflicts with OPTIONS.retry>0") {
        return Some("Choose one: set `no_retry: true` OR set `retry: N` with `no_retry: false`");
    }
    if message.contains("OPTIONS.timeout must be a positive integer") {
        return Some("Set timeout to a positive integer, e.g. `timeout: 30`");
    }
    if message.contains("OPTIONS.retry must be a non-negative integer") {
        return Some("Use `retry: 0` to disable retries or any integer >= 0");
    }
    if message.contains("OPTIONS.retry_delay must be a non-negative number") {
        return Some("Use a non-negative number in seconds, e.g. `retry_delay: 0.5`");
    }
    if message.contains("Attribute #[retry-delay] is deprecated") {
        return Some("Use `#[retry_delay(...)]` instead of `#[retry-delay(...)]`");
    }
    if message.contains("Attribute #[no-retry] is deprecated") {
        return Some("Use `#[no_retry(...)]` instead of `#[no-retry(...)]`");
    }
    if message.contains("Attribute conflict: #[no_retry] with #[retry") {
        return Some("Remove one conflicting attribute to make retry behavior explicit");
    }
    None
}

fn check_preamble_section_order(doc: &parser::GctfDocument) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    let first_body_idx = doc
        .sections
        .iter()
        .position(|s| s.section_type.preamble_rank().is_none())
        .unwrap_or(doc.sections.len());

    let preamble: Vec<_> = doc.sections[..first_body_idx].iter().collect();

    for i in 1..preamble.len() {
        let prev_rank = preamble[i - 1].section_type.preamble_rank().unwrap();
        let curr_rank = preamble[i].section_type.preamble_rank().unwrap();
        if curr_rank < prev_rank {
            let curr_line = preamble[i].start_line + 1;
            let prev_name = preamble[i - 1].section_type.as_str();
            let curr_name = preamble[i].section_type.as_str();
            out.push((
                curr_line,
                format!(
                    "Section order: {} should come before {} (canonical: META→BENCH→ADDRESS→ENDPOINT→TLS→PROTO→OPTIONS)",
                    prev_name, curr_name
                ),
                    format!(
                        "reorder sections so {} comes before {} (or run `grpctestify fmt --write` to auto-fix)",
                        prev_name, curr_name
                    ),
            ));
        }
    }
    out
}

fn check_bench_key_order(doc: &parser::GctfDocument) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for section in &doc.sections {
        if section.section_type == SectionType::Bench {
            if let parser::ast::SectionContent::KeyValues(kv) = &section.content {
                let keys: Vec<_> = kv.keys().collect();
                for i in 1..keys.len() {
                    let prev_rank = bench_key_rank(keys[i - 1]);
                    let curr_rank = bench_key_rank(keys[i]);
                    if curr_rank < prev_rank {
                        let line = section.start_line + 1;
                        out.push((
                            line,
                            format!(
                                "BENCH key order: '{}' should come before '{}' (canonical order via bench_key_rank)",
                                keys[i - 1], keys[i]
                            ),
                        format!(
                            "reorder BENCH keys or run `grpctestify fmt --write` to auto-fix"
                        ),
                        ));
                    }
                }
            }
        }
    }
    out
}

pub async fn handle_check(args: &CheckArgs) -> Result<()> {
    let mut files = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut files_with_errors = 0;

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
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

    sort_and_dedup_files(&mut files);
    sort_diagnostics(&mut diagnostics);

    if files.is_empty() {
        if args.is_json() {
            let summary = build_summary(&diagnostics, files.len(), files_with_errors);
            let report = CheckReport {
                diagnostics,
                summary,
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_text_diagnostics(&diagnostics);
        }

        if files_with_errors > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    info!("Checking {} file(s)...", files.len());

    for file in &files {
        let file_str = file.to_string_lossy().to_string();
        let mut file_has_error = false;
        match parser::parse_gctf(file) {
            Ok(doc) => {
                // Check for deprecated HEADERS using AST section types
                for section in &doc.sections {
                    if doc.section_uses_deprecated_headers_alias(section) {
                        diagnostics.push(
                            Diagnostic::warning(
                                &file_str,
                                "DEPRECATED_SECTION",
                                "HEADERS section is deprecated, use REQUEST_HEADERS instead",
                                section.start_line + 1,
                            )
                            .with_hint("Replace --- HEADERS --- with --- REQUEST_HEADERS ---"),
                        );
                    }
                }

                for (line, msg, hint) in check_preamble_section_order(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(&file_str, "SECTION_ORDER", &msg, line)
                            .with_hint(&hint),
                    );
                }

                for (line, msg, hint) in check_bench_key_order(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(&file_str, "BENCH_KEY_ORDER", &msg, line)
                            .with_hint(&hint),
                    );
                }

                let validation_diagnostics = parser::validate_document_diagnostics(&doc);
                for d in validation_diagnostics {
                    let line = d.line.unwrap_or(1);
                    let mut mapped = match d.severity {
                        ErrorSeverity::Error => {
                            file_has_error = true;
                            Diagnostic::error(&file_str, "VALIDATION_ERROR", &d.message, line)
                        }
                        ErrorSeverity::Warning => {
                            Diagnostic::warning(&file_str, "VALIDATION_WARNING", &d.message, line)
                        }
                        ErrorSeverity::Info => {
                            Diagnostic::info(&file_str, "VALIDATION_INFO", &d.message, line)
                        }
                    };

                    if let Some(hint) = validation_hint(&d.message) {
                        mapped = mapped.with_hint(hint);
                    }
                    diagnostics.push(mapped);
                }

                let semantic_mismatches = semantics::collect_assertion_type_mismatches(&doc);
                for mismatch in semantic_mismatches {
                    diagnostics.push(
                        Diagnostic::error(
                            &file_str,
                            &mismatch.rule_id,
                            &mismatch.message,
                            mismatch.line,
                        )
                        .with_hint(&format!(
                            "Type contract violation in assertion: {}",
                            mismatch.expression
                        )),
                    );
                    file_has_error = true;
                }

                let unknown_plugins = semantics::collect_unknown_plugin_calls(&doc);
                for unknown in unknown_plugins {
                    diagnostics.push(
                        Diagnostic::error(
                            &file_str,
                            &unknown.rule_id,
                            &unknown.message,
                            unknown.line,
                        )
                        .with_hint(&format!("Assertion: {}", unknown.expression)),
                    );
                    file_has_error = true;
                }

                // Validate BENCH section config if --bench flag is set
                if args.bench && !file_has_error {
                    if let Err(e) = crate::commands::bench::validate_bench_config(&doc) {
                        diagnostics.push(Diagnostic::error(
                            &file_str,
                            "BENCH_CONFIG_ERROR",
                            &e.to_string(),
                            1,
                        ));
                        file_has_error = true;
                    }
                }

                if !args.is_json() && !file_has_error {
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
                file_has_error = true;
            }
        }

        if file_has_error {
            files_with_errors += 1;
        }
    }

    if !args.is_json() {
        sort_diagnostics(&mut diagnostics);
        print_text_diagnostics(&diagnostics);
    }

    if args.is_json() {
        sort_diagnostics(&mut diagnostics);
        let summary = build_summary(&diagnostics, files.len(), files_with_errors);
        let report = CheckReport {
            diagnostics,
            summary,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    if files_with_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{GctfDocument, Section, SectionContent, SectionType};
    use std::collections::HashMap;

    fn doc_with_sections(sections: Vec<Section>) -> GctfDocument {
        GctfDocument {
            file_path: "test.gctf".to_string(),
            sections,
            metadata: Default::default(),
            next_document: None,
        }
    }

    fn section(ty: SectionType, kv: &[(&str, &str)]) -> Section {
        let mut content = HashMap::new();
        for (k, v) in kv {
            content.insert(k.to_string(), v.to_string());
        }
        Section {
            section_type: ty,
            content: SectionContent::KeyValues(content),
            inline_options: Default::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
        }
    }

    fn kv_section(ty: SectionType, kv: &[(&str, &str)]) -> Section {
        section(ty, kv)
    }

    #[test]
    fn test_check_preamble_order_clean() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Meta, &[]),
            kv_section(SectionType::Bench, &[("mode", "fixed")]),
            kv_section(SectionType::Address, &[("addr", "localhost")]),
            kv_section(SectionType::Endpoint, &[("ep", "svc/method")]),
            kv_section(SectionType::Options, &[("timeout", "10")]),
        ]);
        let issues = check_preamble_section_order(&doc);
        assert!(
            issues.is_empty(),
            "Expected no ordering issues, got {:?}",
            issues
        );
    }

    #[test]
    fn test_check_preamble_order_violation_options_before_bench() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Options, &[]),
            kv_section(SectionType::Bench, &[("mode", "fixed")]),
        ]);
        let issues = check_preamble_section_order(&doc);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].1.contains("OPTIONS should come before BENCH"));
    }

    #[test]
    fn test_check_preamble_order_violation_address_before_bench() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Address, &[]),
            kv_section(SectionType::Bench, &[]),
        ]);
        let issues = check_preamble_section_order(&doc);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].1.contains("ADDRESS should come before BENCH"));
    }

    #[test]
    fn test_check_preamble_order_multiple_violations() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Options, &[]),
            kv_section(SectionType::Address, &[]),
            kv_section(SectionType::Bench, &[]),
        ]);
        let issues = check_preamble_section_order(&doc);
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn test_check_preamble_order_body_sections_unaffected() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Meta, &[]),
            kv_section(SectionType::Options, &[]),
            Section {
                section_type: SectionType::Request,
                content: SectionContent::Empty,
                ..Default::default()
            },
            Section {
                section_type: SectionType::Response,
                content: SectionContent::Empty,
                ..Default::default()
            },
        ]);
        let issues = check_preamble_section_order(&doc);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_bench_key_order_non_bench_section_ignored() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Options, &[("timeout", "10")]),
            kv_section(SectionType::Bench, &[("mode", "fixed")]),
        ]);
        let issues = check_bench_key_order(&doc);
        assert!(issues.is_empty());
    }
}
