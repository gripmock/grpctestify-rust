// Fmt command - format GCTF files

use anyhow::Result;
use tracing::{error, warn};

use crate::cli::args::FmtArgs;
use crate::optimizer;
use crate::parser;
use crate::semantics;
use crate::utils::FileUtils;

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
    let mut saw_newline_gap = false;

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                push_indent(&mut out, indent);
                saw_newline_gap = false;
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
            saw_newline_gap = false;
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
            saw_newline_gap = false;
            continue;
        }

        if ch == '#' || (ch == '/' && matches!(chars.peek(), Some('/') | Some('*'))) {
            if saw_newline_gap {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                push_indent(&mut out, indent);
            } else if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
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
                out.push('/');
                out.push('/');
                in_line_comment = true;
            }
            saw_newline_gap = false;
            continue;
        }

        match ch {
            '{' | '[' => {
                out.push(ch);
                out.push('\n');
                indent += 1;
                push_indent(&mut out, indent);
                saw_newline_gap = false;
            }
            '}' | ']' => {
                if (ch == '}' && out.ends_with('{')) || (ch == ']' && out.ends_with('[')) {
                    out.push(ch);
                    saw_newline_gap = false;
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
                saw_newline_gap = false;
            }
            ',' => {
                out.push(',');
                out.push('\n');
                push_indent(&mut out, indent);
                saw_newline_gap = false;
            }
            ':' => {
                out.push(':');
                out.push(' ');
                saw_newline_gap = false;
            }
            c if c.is_whitespace() => {
                if c == '\n' {
                    saw_newline_gap = true;
                }
            }
            _ => {
                out.push(ch);
                saw_newline_gap = false;
            }
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

    while output.last().is_some_and(|line| line.trim().is_empty()) {
        output.pop();
    }

    output.push(String::new());
}

fn canonical_line_ending() -> &'static str {
    "\n"
}

fn normalize_eol_for_compare(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains("\r\n") {
        std::borrow::Cow::Owned(s.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn normalize_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| normalize_hash_comment_line(line).unwrap_or_else(|| line.to_string()))
        .collect()
}

fn format_non_json_section_lines(raw: &str) -> Vec<String> {
    normalize_lines(raw)
}

fn format_key_values_section(raw: &str, sort_keys: bool) -> Vec<String> {
    let lines = normalize_lines(raw);

    let mut items: Vec<(usize, String, String)> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        } else if let Some((key, value)) = trimmed.split_once(':') {
            let sort_key = if sort_keys {
                key.trim().to_lowercase()
            } else {
                String::new()
            };
            items.push((
                items.len(),
                sort_key,
                format!("{}: {}", key.trim(), value.trim()),
            ));
        }
    }

    if sort_keys {
        items.sort_by(|a, b| a.1.cmp(&b.1));
    }

    items.into_iter().map(|(_, _, v)| v).collect()
}

fn trim_trailing_blank_lines(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

fn format_section_lines(section: &crate::parser::ast::Section) -> Vec<String> {
    let mut lines = match (&section.section_type, &section.content) {
        (
            crate::parser::ast::SectionType::Request
            | crate::parser::ast::SectionType::Error
            | crate::parser::ast::SectionType::Response,
            crate::parser::ast::SectionContent::Json(value),
        ) => {
            if has_json_style_comments(&section.raw_content) {
                format_json_with_comments(&section.raw_content)
            } else {
                format_json_content(value)
            }
        }
        (
            crate::parser::ast::SectionType::Response,
            crate::parser::ast::SectionContent::JsonLines(values),
        ) => {
            if has_json_style_comments(&section.raw_content) {
                format_json_with_comments(&section.raw_content)
            } else {
                let mut out = Vec::new();
                for value in values {
                    out.extend(format_json_content(value));
                }
                out
            }
        }
        // ASSERTS section uses raw_content directly (optimizer already applied)
        (crate::parser::ast::SectionType::Asserts, _) => {
            return normalize_lines(&section.raw_content);
        }
        (_, crate::parser::ast::SectionContent::KeyValues(_)) => {
            format_key_values_section(&section.raw_content, true)
        }
        (_, crate::parser::ast::SectionContent::Extract(_)) => {
            format_key_values_section(&section.raw_content, true)
        }
        _ => format_non_json_section_lines(&section.raw_content),
    };

    trim_trailing_blank_lines(&mut lines);
    lines
}

/// Format a GCTF document chain via AST.
/// Walks all sections in order. `New` sections become `--- NEW ---` separators.
fn format_gctf_chain(head: &crate::parser::GctfDocument, source: &str) -> String {
    let eol = canonical_line_ending();
    let lines: Vec<&str> = source.lines().collect();
    let mut output: Vec<String> = Vec::new();
    let mut current_line = 0usize;

    // Walk every section across all documents in the chain
    for doc in head.iter_chain() {
        for section in &doc.sections {
            // Interleave comments between previous section end and current section start
            while current_line < section.start_line && current_line < lines.len() {
                output.push(
                    normalize_hash_comment_line(lines[current_line])
                        .unwrap_or_else(|| lines[current_line].to_string()),
                );
                current_line += 1;
            }

            // Normal section
            output.push(section.format_header());
            output.extend(format_section_lines(section));

            current_line = section.end_line.min(lines.len());
            ensure_single_section_separator(&mut output, true);
        }
    }

    // Trailing file lines
    while current_line < lines.len() {
        output.push(
            normalize_hash_comment_line(lines[current_line])
                .unwrap_or_else(|| lines[current_line].to_string()),
        );
        current_line += 1;
    }

    let mut rendered = output.join(eol);
    if !rendered.ends_with(eol) {
        rendered.push_str(eol);
    }
    rendered
}

pub fn format_gctf_content(source: &str, file_name: &str) -> Result<String> {
    let doc = parser::parse_gctf_from_str(source, file_name)?;

    // Apply optimizer rewrites before formatting
    let eol = canonical_line_ending();
    let source_after_optimizer = apply_optimizer_rewrites(&doc, source, eol);

    // Re-parse after optimizer to get updated raw_content
    let doc_after = parser::parse_gctf_from_str(&source_after_optimizer, file_name)?;
    Ok(format_gctf_chain(&doc_after, &source_after_optimizer))
}

/// Apply optimizer rewrites to source lines
fn apply_optimizer_rewrites(doc: &crate::parser::GctfDocument, source: &str, eol: &str) -> String {
    let hints = optimizer::collect_assertion_optimizations(doc);
    if hints.is_empty() {
        return source.to_string();
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

    let mut rewritten = lines.join(eol);
    if source.ends_with('\n') {
        rewritten.push_str(eol);
    }
    rewritten
}

pub async fn handle_fmt(args: &FmtArgs) -> Result<()> {
    let mut files = Vec::new();
    let mut has_error = false;
    let mut files_needing_format = 0usize;

    for path in &args.files {
        if path.is_dir() {
            files.extend(FileUtils::collect_test_files(path, &[]));
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
        let doc = match parser::parse_gctf_from_str(&original, &file_name) {
            Ok(doc) => doc,
            Err(e) => {
                error!("{}:1: [PARSE_ERROR] {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        // Validate each document in the chain
        let mut chain_has_error = false;
        for d in doc.iter_chain() {
            if let Err(e) = parser::validate_document(d) {
                error!("{}:1: [VALIDATION_ERROR] {}", file.display(), e);
                chain_has_error = true;
            }
            for mismatch in semantics::collect_assertion_type_mismatches(d) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    mismatch.line,
                    mismatch.rule_id,
                    mismatch.message
                );
                chain_has_error = true;
            }
            for unknown in semantics::collect_unknown_plugin_calls(d) {
                error!(
                    "{}:{}: [{}] {}",
                    file.display(),
                    unknown.line,
                    unknown.rule_id,
                    unknown.message
                );
                chain_has_error = true;
            }
        }
        if chain_has_error {
            has_error = true;
            continue;
        }

        // Use the format function which handles multi-document automatically
        let formatted = match format_gctf_content(&original, &file_name) {
            Ok(f) => f,
            Err(e) => {
                error!("{}:1: [FORMAT_ERROR] {}", file.display(), e);
                has_error = true;
                continue;
            }
        };

        if args.write {
            // Only write if content changed (idempotent check)
            // Normalize EOL for comparison to handle CRLF input
            let formatted_cmp = normalize_eol_for_compare(&formatted);
            let original_cmp = normalize_eol_for_compare(&original);
            if formatted_cmp != original_cmp
                && let Err(e) = std::fs::write(&file, &formatted)
            {
                error!("Failed to write {}: {}", file.display(), e);
                has_error = true;
            }
        } else {
            let formatted_cmp = normalize_eol_for_compare(&formatted);
            let original_cmp = normalize_eol_for_compare(&original);

            if formatted_cmp != original_cmp {
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
        return Err(anyhow::anyhow!("Formatting failed with errors"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_gctf_content;

    fn to_crlf(input: &str) -> String {
        input.replace('\n', "\r\n")
    }

    #[test]
    fn test_fmt_hash_comments_to_slashes() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE ---
 # Protected behavior: even if a stub tries gripmock -> NOT_SERVING,
# runtime must ignore it and return real status.
{
  "status": "SERVING"
}
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE ---
// Protected behavior: even if a stub tries gripmock -> NOT_SERVING,
// runtime must ignore it and return real status.
{
  "status": "SERVING"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_jsonlines_preserves_comments() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{
  "service": "examples.health.watch"
}

--- RESPONSE with_asserts=true ---
# Delay applies before first message
{
  "status": "NOT_SERVING"
}
// Then service recovers
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{
  "service": "examples.health.watch"
}

--- RESPONSE with_asserts ---
// Delay applies before first message
{
  "status": "NOT_SERVING"
}
// Then service recovers
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
"#;

        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_hash_inside_string_not_comment() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "value": "abc#def"
}

--- RESPONSE ---
{
  "ok": true
}
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "value": "abc#def"
}

--- RESPONSE ---
{
  "ok": true
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_collapses_extra_blank_lines() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}


--- ASSERTS ---
@scope_message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_inserts_blank_line_before_asserts() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}
--- ASSERTS --- 
@scope_message_count() == 2
"#;

        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_crlf_to_lf_between_sections() {
        let source_lf = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}
--- ASSERTS ---
@scope_message_count() == 2
"#;
        let source = to_crlf(source_lf);

        let formatted = format_gctf_content(&source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{
  "service": "gripmock"
}

--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_ends_with_newline() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        assert!(formatted.ends_with('\n'));
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_crlf_to_lf_and_ends_with_newline() {
        let source_lf = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let source = to_crlf(source_lf);
        let formatted = format_gctf_content(&source, "test.gctf").unwrap();
        assert!(formatted.ends_with('\n'));
        assert!(!formatted.contains("\r\n"));
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_address() {
        let source = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_endpoint() {
        let source = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_request() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123,
  "name": "test"
}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{
  "id": 123,
  "name": "test"
}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_inline_option_with_asserts() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok""#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_inline_option_partial() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial=true ---
{
  "id": 1,
  "name": "test",
  "extra": "ignored"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial ---
{
  "extra": "ignored",
  "id": 1,
  "name": "test"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_inline_option_tolerance() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE tolerance=0.01 ---
{
  "value": 3.1415926
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE tolerance=0.01 ---
{
  "value": 3.1415926
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_inline_option_redact() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE redact=["token","secret"] ---
{
  "token": "abc123",
  "secret": "xyz789",
  "public": "visible"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE redact=["secret","token"] ---
{
  "public": "visible",
  "secret": "xyz789",
  "token": "abc123"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_inline_option_unordered_arrays() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE unordered_arrays=true ---
{
  "items": [3, 1, 2]
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE unordered_arrays ---
{
  "items": [
    3,
    1,
    2
  ]
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_response_with_multiple_inline_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true partial=true tolerance=0.1 ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial tolerance=0.1 with_asserts ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_error() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid argument"
}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR ---
{
  "code": 3,
  "message": "Invalid argument"
}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_error_with_inline_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts=true ---
{
  "code": 3
}

--- ASSERTS ---
.code == 3
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts ---
{
  "code": 3
}

--- ASSERTS ---
.code == 3
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_error_with_partial_and_with_asserts_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR with_asserts=true partial=true ---
{
  "code": 5
}

--- ASSERTS ---
.code == 5
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ERROR partial with_asserts ---
{
  "code": 5
}

--- ASSERTS ---
.code == 5
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_request_headers() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Content-Type: application/json
Authorization: Bearer token123

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST_HEADERS ---
Authorization: Bearer token123
Content-Type: application/json

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_asserts() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts=true ---
{
  "status": "ok",
  "count": 42
}

--- ASSERTS ---
.status == "ok"
.count == 42
.count > 10
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE with_asserts ---
{
  "count": 42,
  "status": "ok"
}

--- ASSERTS ---
.status == "ok"
.count == 42
.count > 10
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_proto() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- PROTO ---
files: service.proto
import_path: /proto

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- PROTO ---
files: service.proto
import_path: /proto

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_tls() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
insecure: false
ca_cert: /path/to/ca.crt
server_name: example.com

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- TLS ---
ca_cert: /path/to/ca.crt
insecure: false
server_name: example.com

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_options() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- OPTIONS ---
timeout: 30
dry_run: true
retry_count: 3

--- REQUEST ---
{}

--- RESPONSE ---
{}"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- OPTIONS ---
dry_run: true
retry_count: 3
timeout: 30

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_section_extract() {
        let source = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{
  "id": 123,
  "token": "abc456"
}

--- EXTRACT ---
user_id: .id
auth_token: .token
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{
  "id": 123,
  "token": "abc456"
}

--- EXTRACT ---
auth_token: .token
user_id: .id
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_all_sections_in_order() {
        let source = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST_HEADERS ---
Content-Type: application/json
Authorization: Bearer token

--- PROTO ---
files: health.proto

--- TLS ---
insecure: false

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{
  "service": "grpc.health.v1.Health"
}

--- RESPONSE ---
{
  "status": "SERVING"
}

--- ERROR ---
{
  "code": 5,
  "message": "Service not found"
}

--- ASSERTS ---
.status == "SERVING"

--- EXTRACT ---
status_code: .status
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST_HEADERS ---
Authorization: Bearer token
Content-Type: application/json

--- PROTO ---
files: health.proto

--- TLS ---
insecure: false

--- OPTIONS ---
timeout: 10

--- REQUEST ---
{
  "service": "grpc.health.v1.Health"
}

--- RESPONSE ---
{
  "status": "SERVING"
}

--- ERROR ---
{
  "code": 5,
  "message": "Service not found"
}

--- ASSERTS ---
.status == "SERVING"

--- EXTRACT ---
status_code: .status
"#;
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_fmt_inline_options_all_boolean_combinations() {
        let combinations = [
            ("", ""),
            ("with_asserts=true", "with_asserts"),
            ("partial=true", "partial"),
            ("unordered_arrays=true", "unordered_arrays"),
            ("with_asserts=true partial=true", "partial with_asserts"),
            (
                "with_asserts=true unordered_arrays=true",
                "unordered_arrays with_asserts",
            ),
            (
                "partial=true unordered_arrays=true",
                "partial unordered_arrays",
            ),
            (
                "with_asserts=true partial=true unordered_arrays=true",
                "partial unordered_arrays with_asserts",
            ),
        ];

        for (input_opts, expected_opts) in combinations {
            let source = format!(
                r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{{}}

--- RESPONSE{} ---
{{"status": "ok"}}
"#,
                if input_opts.is_empty() {
                    String::new()
                } else {
                    format!(" {}", input_opts)
                }
            );

            let expected = format!(
                r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{{}}

--- RESPONSE{} ---
{{
  "status": "ok"
}}
"#,
                if expected_opts.is_empty() {
                    String::new()
                } else {
                    format!(" {}", expected_opts)
                }
            );

            let formatted = format_gctf_content(&source, "test.gctf").unwrap();
            assert_eq!(formatted, expected, "Failed for options: {}", input_opts);
        }
    }

    #[test]
    fn test_fmt_detached_comments_preserved() {
        let source = r#"// This is a detached comment before the endpoint
--- ENDPOINT ---
test.Service/Method

// Another detached comment
--- REQUEST ---
{}

// Comment before response
--- RESPONSE ---
{}
"#;
        let formatted = format_gctf_content(source, "test.gctf").unwrap();
        let expected = r#"// This is a detached comment before the endpoint
--- ENDPOINT ---
test.Service/Method

// Another detached comment

--- REQUEST ---
{
}
// Comment before response

--- RESPONSE ---
{}
"#;
        assert_eq!(formatted, expected);
    }
}
