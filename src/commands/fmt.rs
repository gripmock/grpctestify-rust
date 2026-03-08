// Fmt command - format GCTF files

use anyhow::Result;
use tracing::{error, warn};

use crate::cli::args::FmtArgs;
use crate::optimizer;
use crate::parser;
use crate::semantics;
use crate::utils::FileUtils;
use crate::utils::gctf_style::trailing_blank_line_count;

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

fn format_json_content(value: &serde_json::Value) -> Vec<String> {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(str::to_string)
        .collect()
}

fn format_json_with_comments(raw: &str) -> Vec<String> {
    fn push_indent(buf: &mut String, indent: usize) {
        for _ in 0..indent {
            buf.push_str("  ");
        }
    }

    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    let mut indent = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                push_indent(&mut out, indent);
            } else if ch != '\r' {
                out.push(ch);
            }
            continue;
        }

        if in_block_comment {
            out.push(ch);
            if ch == '*'
                && let Some('/') = chars.peek()
            {
                out.push('/');
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }

        if ch == '#' || (ch == '/' && matches!(chars.peek(), Some('/') | Some('*'))) {
            if !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }

            if ch == '/' {
                match chars.peek() {
                    Some('/') => {
                        out.push('/');
                        out.push('/');
                        chars.next();
                        in_line_comment = true;
                    }
                    Some('*') => {
                        out.push('/');
                        out.push('*');
                        chars.next();
                        in_block_comment = true;
                    }
                    _ => out.push('/'),
                }
            } else {
                out.push('#');
                in_line_comment = true;
            }
            continue;
        }

        match ch {
            '{' | '[' => {
                out.push(ch);
                out.push('\n');
                indent += 1;
                push_indent(&mut out, indent);
            }
            '}' | ']' => {
                if (ch == '}' && out.ends_with('{')) || (ch == ']' && out.ends_with('[')) {
                    out.push(ch);
                    continue;
                }
                indent = indent.saturating_sub(1);
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                push_indent(&mut out, indent);
                out.push(ch);
            }
            ',' => {
                out.push(',');
                out.push('\n');
                push_indent(&mut out, indent);
            }
            ':' => {
                out.push(':');
                out.push(' ');
            }
            c if c.is_whitespace() => {}
            _ => out.push(ch),
        }
    }

    out.lines().map(str::to_string).collect()
}

fn has_json_style_comments(raw: &str) -> bool {
    for line in raw.lines() {
        let mut chars = line.chars().peekable();
        let mut in_string = false;
        let mut escaped = false;

        while let Some(ch) = chars.next() {
            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if ch == '"' {
                in_string = !in_string;
                continue;
            }

            if in_string {
                continue;
            }

            if ch == '#' {
                return true;
            }

            if ch == '/'
                && let Some('/') = chars.peek()
            {
                return true;
            }
            if ch == '/'
                && let Some('*') = chars.peek()
            {
                return true;
            }
        }
    }

    false
}

fn ensure_single_section_separator(output: &mut Vec<String>, has_next_section: bool) {
    if !has_next_section {
        return;
    }
    if output.last().is_some_and(|line| !line.trim().is_empty()) {
        output.push(String::new());
    }
}

fn format_gctf_preserve_comments(doc: &crate::parser::GctfDocument, source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut output: Vec<String> = Vec::with_capacity(lines.len());
    let mut current_line = 0usize;

    for (section_idx, section) in doc.sections.iter().enumerate() {
        let has_next_section = section_idx + 1 < doc.sections.len();

        while current_line < section.start_line && current_line < lines.len() {
            if let Some(normalized) = normalize_hash_comment_line(lines[current_line]) {
                output.push(normalized);
            } else {
                output.push(lines[current_line].to_string());
            }
            current_line += 1;
        }

        output.push(section.format_header());
        current_line = current_line.saturating_add(1);

        match (&section.section_type, &section.content) {
            (
                crate::parser::ast::SectionType::Request
                | crate::parser::ast::SectionType::Error
                | crate::parser::ast::SectionType::Response,
                crate::parser::ast::SectionContent::Json(value),
            ) => {
                let content_start = current_line;
                let end = section.end_line.min(lines.len());
                if has_json_style_comments(&section.raw_content) {
                    output.extend(format_json_with_comments(&section.raw_content));
                    let trailing_blanks = trailing_blank_line_count(&lines, content_start, end);
                    let blanks_to_keep = if has_next_section {
                        trailing_blanks.max(1)
                    } else {
                        trailing_blanks
                    };
                    for _ in 0..blanks_to_keep {
                        output.push(String::new());
                    }
                    current_line = end;
                } else {
                    output.extend(format_json_content(value));
                    let trailing_blanks = trailing_blank_line_count(&lines, content_start, end);
                    let blanks_to_keep = if has_next_section {
                        trailing_blanks.max(1)
                    } else {
                        trailing_blanks
                    };
                    for _ in 0..blanks_to_keep {
                        output.push(String::new());
                    }
                    current_line = end;
                }
            }
            (
                crate::parser::ast::SectionType::Response,
                crate::parser::ast::SectionContent::JsonLines(values),
            ) => {
                let content_start = current_line;
                for value in values {
                    output.extend(format_json_content(value));
                }
                let end = section.end_line.min(lines.len());
                let trailing_blanks = trailing_blank_line_count(&lines, content_start, end);
                let blanks_to_keep = if has_next_section {
                    trailing_blanks.max(1)
                } else {
                    trailing_blanks
                };
                for _ in 0..blanks_to_keep {
                    output.push(String::new());
                }
                current_line = end;
            }
            _ => {
                let end = section.end_line.min(lines.len());
                while current_line < end {
                    if let Some(normalized) = normalize_hash_comment_line(lines[current_line]) {
                        output.push(normalized);
                    } else {
                        output.push(lines[current_line].to_string());
                    }
                    current_line += 1;
                }
            }
        }

        ensure_single_section_separator(&mut output, has_next_section);
    }

    while current_line < lines.len() {
        if let Some(normalized) = normalize_hash_comment_line(lines[current_line]) {
            output.push(normalized);
        } else {
            output.push(lines[current_line].to_string());
        }
        current_line += 1;
    }

    let mut rendered = output.join("\n");
    if source.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
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
    let mut files_needing_format = 0usize;

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

        let preformatted = match apply_optimizer_rewrites(&original, &file_name) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to optimize {}: {}", file.display(), e);
                has_error = true;
                continue;
            }
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
        } else {
            if formatted != original {
                println!(
                    "{}:1: [FORMAT_NEEDED] File is not formatted",
                    file.display()
                );
                println!("  hint: Run `grpctestify fmt -w {}`", file.display());
                has_error = true;
                files_needing_format += 1;
            } else {
                println!("{} ... OK", file.display());
            }
        }
    }

    if !args.write && files_needing_format > 0 {
        error!(
            "{} file(s) require formatting. Run `grpctestify fmt -w ...`",
            files_needing_format
        );
    }

    if has_error {
        std::process::exit(1);
    }

    Ok(())
}
