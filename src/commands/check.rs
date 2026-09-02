#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::cli::Cli;
use crate::cli::args::HasFormat;
use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::cli::args::CheckArgs;
use crate::optimizer::OptimizeLevel;
use crate::parser;
use crate::parser::ErrorSeverity;
use crate::report::{CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity, DocumentStructure};
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
        let icon = match d.severity {
            DiagnosticSeverity::Error => crate::report::style::fail_icon().to_string(),
            DiagnosticSeverity::Warning => crate::report::style::warn_icon().to_string(),
            DiagnosticSeverity::Info | DiagnosticSeverity::Hint => {
                crate::report::style::PASS_ICON.to_string()
            }
        };
        println!(
            "{icon} {}:{}: [{}] {}",
            d.file, d.range.start.line, d.code, d.message
        );
        if let Some(hint) = &d.hint {
            println!("    hint: {}", hint);
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

pub fn rhai_plugin_names() -> std::collections::HashSet<String> {
    crate::plugins::rhai_plugin::load_all_configured_plugins()
        .iter()
        .map(|p| p.name().to_string())
        .collect()
}

pub fn rhai_boolean_plugin_names() -> std::collections::HashSet<String> {
    crate::plugins::rhai_plugin::load_all_configured_plugins()
        .iter()
        .filter(|p| {
            let sig = p.signature();
            sig.return_type == crate::plugins::TypeInfo::Bool
                && sig.safe_for_rewrite
                && sig.deterministic
                && sig.idempotent
        })
        .map(|p| p.name().to_string())
        .collect()
}

pub async fn handle_check(args: &CheckArgs, cli: &Cli) -> Result<()> {
    let mut files = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut structure: Vec<DocumentStructure> = Vec::new();
    let mut files_with_errors = 0;
    let mut total_docs_in_suite = 0usize;
    let mut grpc_docs_in_suite = 0usize;
    let mut docs_with_error_section = 0usize;
    let mut docs_with_asserts_section = 0usize;
    let rhai_plugin_names = rhai_plugin_names();
    apif_optimizer::register_extra_boolean_plugins(rhai_boolean_plugin_names());
    crate::parser::register_extra_inline_option_keys(
        crate::plugins::rhai_plugin::load_all_inline_option_keys(),
    );

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
                structure: Vec::new(),
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
                if let Some(source) = doc.metadata.source.as_deref() {
                    for dep in parser::detect_deprecations(&parser::tokenize_gctf(source)) {
                        let line = dep.range.start.line + 1;
                        let (code, hint) = if dep.message.contains("HEADERS") {
                            (
                                "DEPRECATED_SECTION",
                                Some("Replace --- HEADERS --- with --- REQUEST_HEADERS ---"),
                            )
                        } else {
                            ("DEPRECATED_KEY_SPELLING", None)
                        };
                        let mut d = Diagnostic::warning(&file_str, code, &dep.message, line);
                        if let Some(h) = hint {
                            d = d.with_hint(h);
                        }
                        diagnostics.push(d);
                    }
                }

                for (line, msg, hint) in semantics::preamble_section_order(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(&file_str, "SECTION_ORDER", &msg, line)
                            .with_hint(&hint),
                    );
                }

                diagnostics.extend(bench_source_diagnostics(&file_str, &doc));

                let validation_diagnostics = parser::validate_document_chain_diagnostics(&doc);
                for d in validation_diagnostics {
                    let line = d.line.map_or(1, |l| l + 1);
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

                let unknown_plugins =
                    semantics::collect_unknown_plugin_calls_with_extra(&doc, &rhai_plugin_names);
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

                let deprecated_plugins = semantics::collect_deprecated_plugin_calls(&doc);
                for dep in deprecated_plugins {
                    diagnostics.push(
                        Diagnostic::warning(&file_str, &dep.rule_id, &dep.message, dep.line)
                            .with_hint(&format!(
                                "`fmt --write` auto-fixes this to `{}`",
                                dep.replacement
                            )),
                    );
                }

                for constant in semantics::collect_constant_assertions(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            &constant.rule_id,
                            &constant.message,
                            constant.line,
                        )
                        .with_hint(&format!(
                            "Replace with a real check against the response, e.g. `.field {} \"{}\"`",
                            if constant.always { "==" } else { "!=" },
                            "expected_value"
                        )),
                    );
                }

                for dup in semantics::collect_duplicate_assertions(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(&file_str, &dup.rule_id, &dup.message, dup.line)
                            .with_hint("Remove the duplicate or change it to check something else"),
                    );
                }

                for redundant in semantics::collect_redundant_response_assertions(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            &redundant.rule_id,
                            &redundant.message,
                            redundant.line,
                        )
                        .with_hint("Remove this ASSERTS line, or move the field out of RESPONSE if you want ASSERTS to be its only check"),
                    );
                }

                for unused in semantics::collect_unused_variables(&doc) {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "UNUSED_VARIABLE",
                            &semantics::unused_variable_message(&unused),
                            unused.line + 1,
                        )
                        .with_hint(
                            "Remove the unused EXTRACT entry, or reference it via {{var_name}}",
                        ),
                    );
                }

                for diag in doc
                    .metadata
                    .source
                    .as_deref()
                    .map(|src| crate::lsp::handlers::collect_insecure_tls_diagnostics(&doc, src))
                    .unwrap_or_default()
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "TLS_VERIFICATION_SKIPPED",
                            &diag.message,
                            diag.range.start.line as usize + 1,
                        )
                        .with_hint(
                            "Name the CA with `ca_cert:` when the certificate is private, or drop `insecure: true` when it is not",
                        ),
                    );
                }

                for diag in doc
                    .metadata
                    .source
                    .as_deref()
                    .map(|src| crate::lsp::handlers::collect_half_identity_diagnostics(&doc, src))
                    .unwrap_or_default()
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "TLS_CLIENT_IDENTITY_INCOMPLETE",
                            &diag.message,
                            diag.range.start.line as usize + 1,
                        )
                        .with_hint("Name both `client_cert:` and `client_key:`, or neither"),
                    );
                }

                for wrong in doc
                    .metadata
                    .source
                    .as_deref()
                    .map(|src| {
                        crate::lsp::handlers::collect_wrong_family_plugin_diagnostics(
                            src, &file_str,
                        )
                    })
                    .unwrap_or_default()
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "PLUGIN_WRONG_FAMILY",
                            &wrong.message,
                            wrong.range.start.line as usize + 1,
                        )
                        .with_hint(
                            "Assert on the answer this family carries, or move the check to a file of the other family",
                        ),
                    );
                }

                for unhonoured in doc
                    .metadata
                    .source
                    .as_deref()
                    .map(|src| {
                        crate::lsp::handlers::collect_unhonoured_attribute_diagnostics(
                            src, &file_str,
                        )
                    })
                    .unwrap_or_default()
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "ATTRIBUTE_NOT_HONOURED",
                            &unhonoured.message,
                            unhonoured.range.start.line as usize + 1,
                        )
                        .with_hint("Remove it, or write the repetition as separate documents"),
                    );
                }

                for race in crate::lsp::handlers::collect_group_race_diagnostics(&doc) {
                    file_has_error = true;
                    diagnostics.push(
                        Diagnostic::error(
                            &file_str,
                            "PARALLEL_RACE",
                            &race.message,
                            race.range.start.line as usize + 1,
                        )
                        .with_hint(
                            "Move the step out of the group, or bind the value before the group",
                        ),
                    );
                }

                for placeholder in doc
                    .metadata
                    .source
                    .as_deref()
                    .map(crate::lsp::handlers::collect_placeholder_diagnostics)
                    .unwrap_or_default()
                {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            "UNSUBSTITUTED_PLACEHOLDER",
                            &placeholder.message,
                            placeholder.range.start.line as usize + 1,
                        )
                        .with_hint(if placeholder.message.contains("read as paths") {
                            "Write the path itself — a certificate and a schema are read from the directory of the file that names them"
                        } else if placeholder.message.contains("{{dataset.") {
                            "Compare the row's value with a RESPONSE section, which is substituted — or send it in REQUEST or REQUEST_HEADERS"
                        } else {
                            "Use $name for a value from EXTRACT, or move the placeholder into REQUEST, REQUEST_HEADERS, RESPONSE or ERROR"
                        }),
                    );
                }

                for (chain_idx, chain_doc) in doc.iter_chain().enumerate() {
                    structure.push(DocumentStructure {
                        file: file_str.clone(),
                        document_index: chain_idx + 1,
                        sections: chain_doc
                            .sections
                            .iter()
                            .map(|s| s.section_type.as_str().to_string())
                            .collect(),
                    });

                    for section in &chain_doc.sections {
                        if section.section_type == parser::ast::SectionType::Asserts
                            && let parser::ast::SectionContent::Assertions(assertions) =
                                &section.content
                            && assertions.is_empty()
                        {
                            diagnostics.push(
                                Diagnostic::warning(
                                    &file_str,
                                    "EMPTY_ASSERTS",
                                    "ASSERTS section has no assertions",
                                    section.start_line + 1,
                                )
                                .with_hint(
                                    "Add assertion expressions or remove the empty ASSERTS section",
                                ),
                            );
                        }
                    }

                    if let Some(section) =
                        chain_doc.first_section(parser::ast::SectionType::Dataset)
                        && let parser::ast::SectionContent::Rows(rows) = &section.content
                        && rows.len() > 50
                    {
                        diagnostics.push(
                            Diagnostic::info(
                                &file_str,
                                "LARGE_DATASET",
                                &format!(
                                    "DATASET has {} rows — inline data works best for a handful of rows",
                                    rows.len()
                                ),
                                section.start_line + 1,
                            )
                            .with_hint(
                                "Move large datasets to an external CSV/TSV/NDJSON file and use `run --data <file>` instead",
                            ),
                        );
                    }

                    total_docs_in_suite += 1;
                    if chain_doc.transport() == parser::ast::Transport::Grpc {
                        grpc_docs_in_suite += 1;
                    }
                    if chain_doc
                        .first_section(parser::ast::SectionType::Error)
                        .is_some()
                    {
                        docs_with_error_section += 1;
                    }
                    if chain_doc
                        .first_section(parser::ast::SectionType::Asserts)
                        .is_some()
                    {
                        docs_with_asserts_section += 1;
                    }
                }

                let opt_level = cli.optimize_level(OptimizeLevel::Safe);
                let opt_hints = crate::optimizer::collect_assertion_optimizations(&doc, opt_level);
                for hint in opt_hints {
                    diagnostics.push(
                        Diagnostic::warning(
                            &file_str,
                            hint.rule_id.as_str(),
                            &format!("{} → {}", hint.before, hint.after),
                            hint.line,
                        )
                        .with_hint(&hint.preconditions.unwrap_or_default()),
                    );
                }

                if args.bench
                    && !file_has_error
                    && let Err(e) = crate::commands::bench::validate_bench_config(&doc)
                {
                    diagnostics.push(Diagnostic::error(
                        &file_str,
                        "BENCH_CONFIG_ERROR",
                        &e.to_string(),
                        1,
                    ));
                    file_has_error = true;
                }

                if !args.is_json() && !file_has_error {
                    println!(
                        "{} {} ... OK",
                        crate::report::style::pass_icon(),
                        file.display()
                    );
                }
            }
            Err(e) => {
                let said = e.to_string();
                let line = std::fs::read_to_string(file)
                    .ok()
                    .map(|text| {
                        crate::parser::error_recovery::parse_content_with_recovery(&text, &file_str)
                    })
                    .and_then(|recovered| {
                        recovered
                            .diagnostics
                            .diagnostics
                            .iter()
                            .find(|d| said.contains(&d.message) || d.message.contains(&said))
                            .map(|d| d.range.start.line + 1)
                    })
                    .or_else(|| {
                        std::fs::read_to_string(file).ok().and_then(|text| {
                            crate::lsp::handlers::line_of_cause(&text, &said).map(|line| line + 1)
                        })
                    })
                    .unwrap_or(1);
                diagnostics.push(Diagnostic::error(&file_str, "PARSE_ERROR", &said, line));
                file_has_error = true;
            }
        }

        if file_has_error {
            files_with_errors += 1;
        }
    }

    if grpc_docs_in_suite > 0 && docs_with_error_section == 0 {
        diagnostics.push(Diagnostic::info(
            "<suite>",
            "NO_ERROR_CASE_COVERAGE",
            &format!(
                "No test in this suite exercises an ERROR case (0/{grpc_docs_in_suite} documents have an ERROR section)"
            ),
            1,
        ).with_hint("Consider adding at least one test asserting a gRPC error status for the covered services"));
    }

    if total_docs_in_suite > 0 && docs_with_asserts_section == 0 {
        diagnostics.push(Diagnostic::info(
            "<suite>",
            "NO_ASSERTS_COVERAGE",
            &format!(
                "No test in this suite has an ASSERTS section (0/{total_docs_in_suite} documents) — every test relies on exact RESPONSE/ERROR body matching"
            ),
            1,
        ).with_hint("Exact-match RESPONSE/ERROR is a real check, but ASSERTS lets you check specific fields without pinning the whole payload"));
    }

    if !args.is_json() {
        sort_diagnostics(&mut diagnostics);
        print_text_diagnostics(&diagnostics);
        print_check_summary(&diagnostics, files.len(), files_with_errors);
    }

    if args.is_json() {
        sort_diagnostics(&mut diagnostics);
        let summary = build_summary(&diagnostics, files.len(), files_with_errors);
        let report = CheckReport {
            diagnostics,
            summary,
            structure,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    if files_with_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn bench_source_diagnostics(file_str: &str, doc: &parser::GctfDocument) -> Vec<Diagnostic> {
    parser::validator::missing_bench_source_files(doc)
        .into_iter()
        .map(|d| {
            Diagnostic::warning(
                file_str,
                "VALIDATION_WARNING",
                &d.message,
                d.line.map_or(1, |l| l + 1),
            )
        })
        .collect()
}

fn print_check_summary(diagnostics: &[Diagnostic], total_files: usize, files_with_errors: usize) {
    use crate::report::style::{
        dim_style, fail_icon, fail_style, pass_icon, pass_style, warn_style,
    };
    let errors = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, DiagnosticSeverity::Warning))
        .count();

    println!();
    if files_with_errors == 0 {
        println!(
            "{} {} {}",
            pass_icon(),
            pass_style().apply_to("PASSED"),
            dim_style().apply_to(format!("· {total_files} files checked"))
        );
    } else {
        let mut parts = vec![
            fail_style()
                .apply_to(format!("{errors} errors"))
                .to_string(),
        ];
        if warnings > 0 {
            parts.push(
                warn_style()
                    .apply_to(format!("{warnings} warnings"))
                    .to_string(),
            );
        }
        println!(
            "{} {} {}",
            fail_icon(),
            fail_style().apply_to("FAILED"),
            dim_style().apply_to(format!(
                "· {} · {files_with_errors}/{total_files} files",
                parts.join(", ")
            ))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::bench_source_diagnostics;
    use crate::parser::ast::{GctfDocument, Section, SectionContent, SectionSpan, SectionType};
    use crate::semantics;

    fn doc_with_sections(sections: Vec<Section>) -> GctfDocument {
        GctfDocument {
            file_path: "test.gctf".to_string(),
            sections,
            metadata: Default::default(),
            next_document: None,
        }
    }

    fn section(ty: SectionType, kv: &[(&str, &str)]) -> Section {
        let mut content = crate::parser::OrderedStringMap::new();
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
            span: SectionSpan::default(),
        }
    }

    fn kv_section(ty: SectionType, kv: &[(&str, &str)]) -> Section {
        section(ty, kv)
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn a_missing_bench_source_is_reported_on_the_bench_header_line_one_based() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- BENCH ---\nsources:\n  - name: users\n    file: nowhere/users.csv\n\n--- REQUEST ---\n{}\n";
        let doc = crate::parser::parse_gctf_from_str(src, "b.gctf").expect("parse");
        let found = bench_source_diagnostics("b.gctf", &doc);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].range.start.line, 4,
            "1-based line of `--- BENCH ---`"
        );
        assert!(
            found[0].message.contains("nowhere/users.csv"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn check_preamble_order_clean() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Meta, &[]),
            kv_section(SectionType::Bench, &[("mode", "fixed")]),
            kv_section(SectionType::Address, &[("addr", "localhost")]),
            kv_section(SectionType::Endpoint, &[("ep", "svc/method")]),
            kv_section(SectionType::Options, &[("timeout", "10")]),
        ]);
        let issues = semantics::preamble_section_order(&doc);
        assert!(
            issues.is_empty(),
            "Expected no ordering issues, got {:?}",
            issues
        );
    }

    #[test]
    fn check_preamble_order_violation_options_before_bench() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Options, &[]),
            kv_section(SectionType::Bench, &[("mode", "fixed")]),
        ]);
        let issues = semantics::preamble_section_order(&doc);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].1.contains("BENCH should come before OPTIONS"));
    }

    #[test]
    fn check_preamble_order_violation_address_before_bench() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Address, &[]),
            kv_section(SectionType::Bench, &[]),
        ]);
        let issues = semantics::preamble_section_order(&doc);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].1.contains("BENCH should come before ADDRESS"));
    }

    #[test]
    fn check_preamble_order_multiple_violations() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Options, &[]),
            kv_section(SectionType::Address, &[]),
            kv_section(SectionType::Bench, &[]),
        ]);
        let issues = semantics::preamble_section_order(&doc);
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn check_preamble_order_body_sections_unaffected() {
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
        let issues = semantics::preamble_section_order(&doc);
        assert!(issues.is_empty());
    }

    #[test]
    fn section_order_hint_promises_fmt_autofix() {
        let doc = doc_with_sections(vec![
            kv_section(SectionType::Address, &[]),
            kv_section(SectionType::Bench, &[]),
        ]);
        let issues = semantics::preamble_section_order(&doc);
        let hint = &issues[0].2;
        assert!(
            hint.contains("fmt --write"),
            "fmt now reorders preamble sections for real: {hint}"
        );
    }
}
