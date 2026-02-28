// Check command - validate GCTF files

use anyhow::Result;
use tracing::info;

use crate::cli::args::CheckArgs;
use crate::parser;
use crate::report::{CheckReport, CheckSummary, Diagnostic, DiagnosticSeverity};
use crate::utils::FileUtils;

pub async fn handle_check(args: &CheckArgs) -> Result<()> {
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
            let total_errors = diagnostics
                .iter()
                .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
                .count();
            let total_warnings = diagnostics
                .iter()
                .filter(|d| matches!(d.severity, DiagnosticSeverity::Warning))
                .count();
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
        let total_errors = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
            .count();
        let total_warnings = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Warning))
            .count();
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
