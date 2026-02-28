// Fmt command - format GCTF files

use anyhow::Result;
use tracing::{error, warn};

use crate::cli::args::FmtArgs;
use crate::optimizer;
use crate::parser;
use crate::semantics;
use crate::utils::FileUtils;

fn parse_header_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if !trimmed.starts_with("---") || !trimmed.ends_with("---") || trimmed.len() < 6 {
        return None;
    }

    let inner = trimmed[3..trimmed.len() - 3].trim();
    if inner.is_empty() {
        return None;
    }

    let mut parts = inner.splitn(2, char::is_whitespace);
    let keyword = parts.next()?;
    let rest = parts.next().map(str::trim).unwrap_or("");

    Some((keyword, rest))
}

fn normalize_header_line(canonical_section: &str, rest: &str) -> String {
    if rest.is_empty() {
        format!("--- {} ---", canonical_section)
    } else {
        format!("--- {} {} ---", canonical_section, rest)
    }
}

fn normalize_hash_comment_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let rest = trimmed.trim_start_matches('#').trim_start();

    if rest.is_empty() {
        Some(format!("{}//", indent))
    } else {
        Some(format!("{}// {}", indent, rest))
    }
}

fn format_gctf_preserve_comments(doc: &crate::parser::GctfDocument, source: &str) -> String {
    let mut section_idx = 0usize;
    let mut output = String::with_capacity(source.len());

    for chunk in source.split_inclusive('\n') {
        let (line, newline) = if let Some(stripped) = chunk.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (chunk, "")
        };

        if section_idx < doc.sections.len()
            && let Some((keyword, rest)) = parse_header_line(line)
            && let Some(actual) = crate::parser::ast::SectionType::from_keyword(keyword)
            && actual == doc.sections[section_idx].section_type
        {
            let normalized =
                normalize_header_line(doc.sections[section_idx].section_type.as_str(), rest);
            output.push_str(&normalized);
            section_idx += 1;
        } else if let Some(normalized) = normalize_hash_comment_line(line) {
            output.push_str(&normalized);
        } else {
            output.push_str(line);
        }
        output.push_str(newline);
    }

    output
}

pub fn format_gctf_content(source: &str, file_name: &str) -> Result<String> {
    let doc = parser::parse_gctf_from_str(source, file_name)?;
    Ok(format_gctf_preserve_comments(&doc, source))
}

fn apply_optimizer_rewrites(source: &str, file_name: &str) -> Result<String> {
    let doc = parser::parse_gctf_from_str(source, file_name)?;
    let hints = optimizer::collect_assertion_optimizations(&doc);
    if hints.is_empty() {
        return Ok(source.to_string());
    }

    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
    for hint in hints {
        let line_idx = hint.line.saturating_sub(1);
        if line_idx >= lines.len() {
            continue;
        }

        let line = &mut lines[line_idx];
        if let Some(start) = line.find(&hint.before) {
            let end = start + hint.before.len();
            line.replace_range(start..end, &hint.after);
        }
    }

    let mut rewritten = lines.join("\n");
    if source.ends_with('\n') {
        rewritten.push('\n');
    }
    Ok(rewritten)
}

fn collect_check_errors_for_content(
    source: &str,
    file_name: &str,
) -> Vec<(usize, String, String, Option<String>)> {
    let mut errors = Vec::new();

    let doc = match parser::parse_gctf_from_str(source, file_name) {
        Ok(doc) => doc,
        Err(e) => {
            errors.push((1, "PARSE_ERROR".to_string(), e.to_string(), None));
            return errors;
        }
    };

    if let Err(e) = parser::validate_document(&doc) {
        errors.push((1, "VALIDATION_ERROR".to_string(), e.to_string(), None));
    }

    for mismatch in semantics::collect_assertion_type_mismatches(&doc) {
        errors.push((
            mismatch.line,
            mismatch.rule_id,
            mismatch.message,
            Some(format!(
                "Type contract violation in assertion: {}",
                mismatch.expression
            )),
        ));
    }

    for unknown in semantics::collect_unknown_plugin_calls(&doc) {
        errors.push((
            unknown.line,
            unknown.rule_id,
            unknown.message,
            Some(format!("Assertion: {}", unknown.expression)),
        ));
    }

    errors
}

pub async fn handle_fmt(args: &FmtArgs) -> Result<()> {
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
        let original = match std::fs::read_to_string(&file) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        let file_name = file.to_string_lossy();
        let check_errors = collect_check_errors_for_content(&original, &file_name);
        if !check_errors.is_empty() {
            for (line, code, message, hint) in check_errors {
                error!("{}:{}: [{}] {}", file.display(), line, code, message);
                if let Some(hint) = hint {
                    error!("  hint: {}", hint);
                }
            }
            has_error = true;
            continue;
        }

        let preformatted = if args.optimize {
            match apply_optimizer_rewrites(&original, &file_name) {
                Ok(content) => content,
                Err(e) => {
                    error!("Failed to optimize {}: {}", file.display(), e);
                    has_error = true;
                    continue;
                }
            }
        } else {
            original.clone()
        };

        let formatted = match format_gctf_content(&preformatted, &file_name) {
            Ok(formatted) => formatted,
            Err(e) => {
                error!("Failed to parse {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        if args.write {
            // Only write if content changed (idempotent check)
            if formatted != original
                && let Err(e) = std::fs::write(&file, &formatted)
            {
                error!("Failed to write {}: {}", file.display(), e);
                has_error = true;
            }
            // Silent success - standard fmt behavior
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
