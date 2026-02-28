// GCTF file parser - converts .gctf text to AST
// Handles section extraction, comment removal, and inline option parsing

use super::ast::*;
use super::json_mod;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

static SECTION_HEADER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(SECTION_HEADER_PATTERN).expect("invalid section header regex"));

/// Section header pattern: --- SECTION_NAME key=value ... ---
const SECTION_HEADER_PATTERN: &str = r"^---\s*([A-Z_]+)(\s+.+)?\s*---$";

/// Parse a .gctf file into an AST
pub fn parse_gctf(file_path: &Path) -> Result<GctfDocument> {
    let (document, _) = parse_gctf_with_diagnostics(file_path)?;
    Ok(document)
}

/// Parse .gctf content from string (for LSP/editor use)
pub fn parse_gctf_from_str(content: &str, file_path: &str) -> Result<GctfDocument> {
    let source_lines: Vec<&str> = content.lines().collect();
    let mut document = GctfDocument::new(file_path.to_string());
    document.metadata.source = Some(content.to_string());
    let (sections, _) = parse_sections(&source_lines)?;
    document.sections = sections;
    Ok(document)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParseTimings {
    pub read_ms: f64,
    pub parse_sections_ms: f64,
    pub build_document_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParseDiagnostics {
    pub file_path: String,
    pub bytes: usize,
    pub total_lines: usize,
    pub section_headers: usize,
    pub section_counts: HashMap<String, usize>,
    pub timings: ParseTimings,
}

/// Parse .gctf and return AST + diagnostics useful for inspect/debug
pub fn parse_gctf_with_diagnostics(file_path: &Path) -> Result<(GctfDocument, ParseDiagnostics)> {
    let total_start = Instant::now();

    // Read file content
    let read_start = Instant::now();
    let source = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
    let read_ms = read_start.elapsed().as_secs_f64() * 1000.0;

    let source_lines: Vec<&str> = source.lines().collect();

    // Initialize document
    let init_start = Instant::now();
    let mut document = GctfDocument::new(file_path.display().to_string());
    document.metadata.source = Some(source.clone());
    let init_ms = init_start.elapsed().as_secs_f64() * 1000.0;

    // Parse sections
    let parse_sections_start = Instant::now();
    let (sections, section_headers) = parse_sections(&source_lines)?;
    let parse_sections_ms = parse_sections_start.elapsed().as_secs_f64() * 1000.0;

    let attach_start = Instant::now();
    document.sections = sections;
    let attach_ms = attach_start.elapsed().as_secs_f64() * 1000.0;

    // Non-overlapping "document build" time (without section parsing itself)
    let build_document_ms = init_ms + attach_ms;
    let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    let mut section_counts: HashMap<String, usize> = HashMap::new();
    for section in &document.sections {
        *section_counts
            .entry(section.section_type.as_str().to_string())
            .or_insert(0) += 1;
    }

    let diagnostics = ParseDiagnostics {
        file_path: file_path.display().to_string(),
        bytes: source.len(),
        total_lines: source_lines.len(),
        section_headers,
        section_counts,
        timings: ParseTimings {
            read_ms,
            parse_sections_ms,
            build_document_ms,
            total_ms,
        },
    };

    Ok((document, diagnostics))
}

/// Parse all sections from lines
fn parse_sections(lines: &[&str]) -> Result<(Vec<Section>, usize)> {
    let mut sections = Vec::new();
    let mut section_headers = 0;
    let mut current_section: Option<(SectionType, usize, Vec<String>, InlineOptions)> = None;
    let header_regex = &*SECTION_HEADER_REGEX;

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check for section header
        if let Some(captures) = header_regex.captures(trimmed) {
            // Save previous section
            if let Some((section_type, start_line, content, options)) = current_section.take() {
                let section = build_section(section_type, start_line, line_idx, &content, options)?;
                sections.push(section);
            }

            section_headers += 1;

            // Start new section
            let section_name = captures.get(1).unwrap().as_str();
            let inline_options_str = captures.get(2).map(|m| m.as_str());

            if let Some(section_type) = SectionType::from_keyword(section_name) {
                let inline_options = if let Some(opts_str) = inline_options_str {
                    parse_inline_options(opts_str)?
                } else {
                    InlineOptions::default()
                };

                // Store with inline options flag
                // We'll parse content after section header
                current_section = Some((section_type, line_idx, Vec::new(), inline_options));
            } else {
                return Err(anyhow::anyhow!("Unknown section type: {}", section_name));
            }

            continue;
        }

        // Add content to current section
        if let Some((_, _, ref mut content, _)) = current_section {
            content.push(line.to_string());
        }
    }

    // Save last section
    if let Some((section_type, start_line, content, options)) = current_section {
        let end_line = lines.len();
        let section = build_section(section_type, start_line, end_line, &content, options)?;
        sections.push(section);
    }

    Ok((sections, section_headers))
}

/// Build a section from parsed content
fn build_section(
    section_type: SectionType,
    start_line: usize,
    end_line: usize,
    content: &[String],
    inline_options: InlineOptions,
) -> Result<Section> {
    // For raw content, we want to preserve indentation but trim empty lines at start/end
    // and maybe trim common indentation if possible?
    // For now, let's just join the lines. The user likely indented them relative to the file.
    // However, build_section receives lines that might have file-level indentation.
    // But .gctf files usually have sections at top level.
    let raw_content = content.join("\n");

    // Remove comments and whitespace for parsing logic (if needed by specific parsers)
    // Actually parse_section_content uses cleaned_content.
    let cleaned_content: String = content
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    // Get section content type and parse
    let section_content = parse_section_content(section_type, &cleaned_content)?;

    // Use the passed inline_options instead of trying to parse again
    // Previously we tried to parse from content.first(), which was wrong.

    Ok(Section {
        section_type,
        content: section_content,
        inline_options,
        raw_content,
        start_line,
        end_line,
    })
}

/// Parse key=value options from string
fn parse_key_value_options(s: &str) -> Result<HashMap<String, String>> {
    let mut options = HashMap::new();
    let tokens = tokenize_options(s)?;

    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            options.insert(key, value);
        }
    }

    Ok(options)
}

/// Tokenize options string, respecting quotes
fn tokenize_options(s: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in s.chars() {
        match (ch, in_quotes, escaped) {
            ('\\', _, false) => {
                escaped = true;
                current_token.push(ch);
            }
            (_, _, true) => {
                escaped = false;
                current_token.push(ch);
            }
            ('"', false, _) => {
                in_quotes = true;
                current_token.push(ch);
            }
            ('"', true, _) => {
                in_quotes = false;
                current_token.push(ch);
            }
            (' ', false, _) => {
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
            }
            _ => {
                current_token.push(ch);
            }
        }
    }

    if !current_token.is_empty() {
        tokens.push(current_token);
    }

    Ok(tokens)
}

/// Parse inline options from header
fn parse_inline_options(s: &str) -> Result<InlineOptions> {
    let options = parse_key_value_options(s)?;

    let mut inline_options = InlineOptions::default();

    if let Some(with_asserts) = options.get("with_asserts") {
        inline_options.with_asserts = matches!(with_asserts.as_str(), "true" | "1");
    }

    if let Some(partial) = options.get("partial") {
        inline_options.partial = matches!(partial.as_str(), "true" | "1");
    }

    if let Some(tolerance) = options.get("tolerance")
        && let Ok(t) = tolerance.parse::<f64>()
    {
        inline_options.tolerance = Some(t);
    }

    if let Some(redact) = options.get("redact") {
        // Parse array format: ["field1","field2"]
        let redact_str = redact.trim().trim_matches('[').trim_matches(']');
        let strings: Vec<String> = redact_str
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        inline_options.redact = strings;
    }

    if let Some(unnamed_arrays) = options.get("unordered_arrays") {
        inline_options.unordered_arrays = matches!(unnamed_arrays.as_str(), "true" | "1");
    }

    Ok(inline_options)
}

/// Parse section content based on section type
fn parse_section_content(section_type: SectionType, content: &str) -> Result<SectionContent> {
    let content = content.trim();

    if content.is_empty() {
        return Ok(SectionContent::Empty);
    }

    match section_type {
        // Single value sections
        SectionType::Address | SectionType::Endpoint => {
            Ok(SectionContent::Single(content.to_string()))
        }

        // JSON sections
        SectionType::Request | SectionType::Error => {
            let json_value = json_mod::from_str(content)?;
            Ok(SectionContent::Json(json_value))
        }
        SectionType::Response => {
            // Primary mode: a single JSON/JSON5 value
            if let Ok(json_value) = json_mod::from_str(content) {
                return Ok(SectionContent::Json(json_value));
            }

            // Legacy-compatible mode: newline-delimited JSON objects within one RESPONSE block
            // Example:
            //   { ... }
            //   { ... }
            let mut values = Vec::new();
            let mut all_lines_json = true;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match json_mod::from_str(trimmed) {
                    Ok(v) => values.push(v),
                    Err(_) => {
                        all_lines_json = false;
                        break;
                    }
                }
            }

            if all_lines_json && values.len() >= 2 {
                Ok(SectionContent::JsonLines(values))
            } else {
                // Preserve original parse error behavior for malformed single-content responses
                let json_value = json_mod::from_str(content)?;
                Ok(SectionContent::Json(json_value))
            }
        }

        // Key-value sections
        SectionType::RequestHeaders
        | SectionType::Tls
        | SectionType::Proto
        | SectionType::Options => {
            let key_values = parse_key_value_section(content)?;
            Ok(SectionContent::KeyValues(key_values))
        }

        // Extract section - support ternary expressions via AST
        SectionType::Extract => {
            let mut key_values = HashMap::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }

                // Parse using ternary AST
                if let Some(extract_var) = crate::parser::ternary_ast::ExtractVar::parse(trimmed) {
                    // Store the JQ-converted value for backward compatibility
                    key_values.insert(extract_var.name, extract_var.value.to_jq());
                }
            }
            Ok(SectionContent::Extract(key_values))
        }

        // Assertion sections
        SectionType::Asserts => {
            let assertions = parse_assertions(content)?;
            Ok(SectionContent::Assertions(assertions))
        }
    }
}

/// Parse key-value section (one per line: key: value)
fn parse_key_value_section(content: &str) -> Result<HashMap<String, String>> {
    let mut key_values = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse key: value
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            key_values.insert(key, value);
        }
    }

    Ok(key_values)
}

/// Parse assertions section (one assertion per line)
fn parse_assertions(content: &str) -> Result<Vec<String>> {
    let assertions: Vec<String> = content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| normalize_regex_literals(&line))
        .collect();

    Ok(assertions)
}

/// Normalize JavaScript-style regex literals /pattern/ to regular strings
/// Converts: @regex(.field, /\d{4}/) → @regex(.field, "\d{4}")
fn normalize_regex_literals(line: &str) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut string_char = None;

    while let Some(c) = chars.next() {
        // Track string literals (single or double quotes)
        if (c == '"' || c == '\'') && !in_string {
            in_string = true;
            string_char = Some(c);
            result.push(c);
        } else if in_string && Some(c) == string_char {
            // Check for escape
            if result.ends_with('\\') {
                result.push(c);
            } else {
                in_string = false;
                string_char = None;
                result.push(c);
            }
        } else if !in_string && c == '/' {
            // Found potential regex literal start
            let mut regex_content = String::new();
            let mut found_end = false;

            // Collect content until closing /
            while let Some(&next_c) = chars.peek() {
                if next_c == '/' {
                    chars.next(); // consume closing /
                    found_end = true;
                    break;
                }
                regex_content.push(chars.next().unwrap());
            }

            if found_end {
                // Convert /pattern/ to "pattern" (even if pattern is empty)
                result.push('"');
                result.push_str(&regex_content);
                result.push('"');
            } else {
                // Not a valid regex literal, keep the /
                result.push('/');
                result.push_str(&regex_content);
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyfill::runtime;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tokenize_options() {
        let result = tokenize_options("key1=value1 key2=value2").unwrap();
        assert_eq!(result, vec!["key1=value1", "key2=value2"]);
    }

    #[test]
    fn test_tokenize_options_with_quotes() {
        let result = tokenize_options(r#"key="value with spaces""#).unwrap();
        assert_eq!(result, vec![r#"key="value with spaces""#]);
    }

    #[test]
    fn test_tokenize_options_empty() {
        let result = tokenize_options("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_tokenize_options_single() {
        let result = tokenize_options("key=value").unwrap();
        assert_eq!(result, vec!["key=value"]);
    }

    #[test]
    fn test_parse_key_value_options() {
        let result = parse_key_value_options("key1=value1 key2=value2").unwrap();
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_parse_key_value_options_empty() {
        let result = parse_key_value_options("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_inline_options() {
        let result = parse_inline_options("with_asserts=true partial=false tolerance=0.1").unwrap();
        assert!(result.with_asserts);
        assert!(!result.partial);
        assert_eq!(result.tolerance, Some(0.1));
    }

    #[test]
    fn test_parse_inline_options_partial() {
        let result = parse_inline_options("partial=true").unwrap();
        assert!(result.partial);
        assert!(!result.with_asserts);
        assert!(result.tolerance.is_none());
    }

    #[test]
    fn test_parse_inline_options_redact() {
        let result = parse_inline_options("redact=password,token").unwrap();
        assert_eq!(result.redact, vec!["password", "token"]);
    }

    #[test]
    fn test_parse_inline_options_unordered_arrays() {
        let result = parse_inline_options("unordered_arrays=true").unwrap();
        assert!(result.unordered_arrays);
    }

    #[test]
    fn test_parse_inline_options_empty() {
        let result = parse_inline_options("").unwrap();
        assert!(!result.with_asserts);
        assert!(!result.partial);
        assert!(result.tolerance.is_none());
        assert!(result.redact.is_empty());
        assert!(!result.unordered_arrays);
    }

    #[test]
    fn test_parse_inline_options_invalid_tolerance() {
        let result = parse_inline_options("tolerance=invalid").unwrap();
        assert!(result.tolerance.is_none());
    }

    #[test]
    fn test_parse_key_value_section() {
        let content = r#"
# Comment
key1: value1
key2: value2
"#;
        let result = parse_key_value_section(content).unwrap();
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));
        assert!(!result.contains_key("#"));
    }

    #[test]
    fn test_parse_key_value_section_empty() {
        let content = "";
        let result = parse_key_value_section(content).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_key_value_section_colon_space() {
        let content = "key:value";
        let result = parse_key_value_section(content).unwrap();
        assert_eq!(result.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_parse_assertions() {
        let content = r#"
.status == "success"
.data | length > 0
"#;
        let result = parse_assertions(content).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&".status == \"success\"".to_string()));
    }

    #[test]
    fn test_parse_assertions_empty() {
        let content = "";
        let result = parse_assertions(content).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_assertions_with_comments() {
        let content = r#"
# This is a comment
.status == 200
"#;
        let result = parse_assertions(content).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&".status == 200".to_string()));
    }

    #[test]
    fn test_parse_gctf_from_str() {
        let content = r#"--- ENDPOINT ---
Service/Method

--- REQUEST ---
{"key": "value"}

--- RESPONSE ---
{"result": "ok"}
"#;
        let document = parse_gctf_from_str(content, "test.gctf").unwrap();
        assert_eq!(document.file_path, "test.gctf");
        assert_eq!(document.sections.len(), 3);
    }

    #[test]
    fn test_parse_gctf_from_str_empty() {
        let content = "";
        let document = parse_gctf_from_str(content, "test.gctf").unwrap();
        assert_eq!(document.file_path, "test.gctf");
        assert!(document.sections.is_empty());
    }

    #[test]
    fn test_parse_gctf() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let content = r#"--- ENDPOINT ---
Service/Method

--- REQUEST ---
{"key": "value"}
"#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(content.as_bytes()).unwrap();

        let document = parse_gctf(temp_file.path()).unwrap();
        assert_eq!(document.sections.len(), 2);
    }

    #[test]
    fn test_parse_gctf_nonexistent_file() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let result = parse_gctf(Path::new("/nonexistent/file.gctf"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gctf_with_diagnostics() {
        if !runtime::supports(runtime::Capability::IsolatedFsIo) {
            return;
        }

        let content = r#"--- ENDPOINT ---
Service/Method

--- REQUEST ---
{"key": "value"}

--- RESPONSE ---
{"result": "ok"}
"#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(content.as_bytes()).unwrap();

        let (document, diagnostics) = parse_gctf_with_diagnostics(temp_file.path()).unwrap();
        assert_eq!(document.sections.len(), 3);
        assert!(diagnostics.bytes > 0);
        assert_eq!(diagnostics.section_headers, 3);
        assert!(diagnostics.timings.total_ms > 0.0);
    }

    #[test]
    fn test_parse_timings_debug() {
        let timings = ParseTimings {
            read_ms: 1.0,
            parse_sections_ms: 2.0,
            build_document_ms: 3.0,
            total_ms: 4.0,
        };
        let debug_str = format!("{:?}", timings);
        assert!(debug_str.contains("ParseTimings"));
    }

    #[test]
    fn test_parse_diagnostics_debug() {
        let diagnostics = ParseDiagnostics {
            file_path: "test.gctf".to_string(),
            bytes: 100,
            total_lines: 10,
            section_headers: 3,
            section_counts: HashMap::new(),
            timings: ParseTimings {
                read_ms: 1.0,
                parse_sections_ms: 2.0,
                build_document_ms: 3.0,
                total_ms: 4.0,
            },
        };
        let debug_str = format!("{:?}", diagnostics);
        assert!(debug_str.contains("ParseDiagnostics"));
        assert!(debug_str.contains("test.gctf"));
    }

    #[test]
    fn test_normalize_regex_literals_js_style() {
        // JavaScript-style regex literals
        let input = r#"@regex(.field, /\d{4}/)"#;
        let output = normalize_regex_literals(input);
        assert_eq!(output, r#"@regex(.field, "\d{4}")"#);
    }

    #[test]
    fn test_normalize_regex_literals_complex() {
        // Complex regex pattern
        let input = r#"@regex(.email, /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/)"#;
        let output = normalize_regex_literals(input);
        assert_eq!(
            output,
            r#"@regex(.email, "^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")"#
        );
    }

    #[test]
    fn test_normalize_regex_literals_preserves_strings() {
        // Regular strings should not be affected
        let input = r#"@regex(.field, "pattern")"#;
        let output = normalize_regex_literals(input);
        assert_eq!(output, r#"@regex(.field, "pattern")"#);
    }

    #[test]
    fn test_normalize_regex_literals_mixed() {
        // Mixed: string and regex literal
        let input = r#"@regex(.field1, "pattern1") and @regex(.field2, /\d+/)"#;
        let output = normalize_regex_literals(input);
        assert_eq!(
            output,
            r#"@regex(.field1, "pattern1") and @regex(.field2, "\d+")"#
        );
    }

    #[test]
    fn test_normalize_regex_literals_division_preserved() {
        // Division operator should be preserved (not a regex)
        let input = r#"100 / 5"#;
        let output = normalize_regex_literals(input);
        assert_eq!(output, r#"100 / 5"#);
    }

    #[test]
    fn test_normalize_regex_literals_empty() {
        // Empty regex literal - should be preserved as-is (invalid pattern)
        let input = r#"@regex(.field, //)"#;
        let output = normalize_regex_literals(input);
        // Empty regex becomes empty string ""
        assert_eq!(output, r#"@regex(.field, "")"#);
    }

    #[test]
    fn test_normalize_regex_literals_escaped_quotes() {
        // Escaped quotes in strings
        let input = r#"@regex(.field, "test \"quoted\"")"#;
        let output = normalize_regex_literals(input);
        assert_eq!(output, r#"@regex(.field, "test \"quoted\"")"#);
    }
}
