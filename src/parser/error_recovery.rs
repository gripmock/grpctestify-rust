// Error recovery parser for GCTF files
// Parses as much as possible and collects all errors

use crate::diagnostics::{DiagnosticCode, DiagnosticCollection, Range};
use crate::parser::ast::{DocumentMetadata, GctfDocument, Section, SectionContent, SectionType};
use std::path::Path;

/// Result of error recovery parsing
pub struct ErrorRecoveryResult {
    pub document: GctfDocument,
    pub diagnostics: DiagnosticCollection,
    pub recovered_sections: usize,
    pub failed_sections: usize,
}

/// Parse GCTF file with error recovery
pub fn parse_with_recovery(file_path: &Path) -> ErrorRecoveryResult {
    let content = std::fs::read_to_string(file_path).unwrap_or_default();
    parse_content_with_recovery(&content, file_path.to_string_lossy().as_ref())
}

/// Parse GCTF content string with error recovery
pub fn parse_content_with_recovery(content: &str, file_path: &str) -> ErrorRecoveryResult {
    let mut diagnostics = DiagnosticCollection::new();
    let mut sections = Vec::new();
    let mut recovered_sections = 0;
    let mut failed_sections = 0;

    let lines: Vec<&str> = content.lines().collect();
    let mut current_line = 0;

    // Parse sections one by one, collecting errors
    while current_line < lines.len() {
        match parse_section(&lines, current_line, &mut diagnostics) {
            Ok((section, end_line)) => {
                sections.push(section);
                recovered_sections += 1;
                current_line = end_line;
            }
            Err(end_line) => {
                failed_sections += 1;
                current_line = end_line;
            }
        }
    }

    let document = GctfDocument {
        file_path: file_path.to_string(),
        sections,
        metadata: DocumentMetadata {
            source: Some(content.to_string()),
            mtime: None,
            parsed_at: 0,
        },
    };

    ErrorRecoveryResult {
        document,
        diagnostics,
        recovered_sections,
        failed_sections,
    }
}

/// Parse a single section from lines
fn parse_section(
    lines: &[&str],
    start_line: usize,
    diagnostics: &mut DiagnosticCollection,
) -> Result<(Section, usize), usize> {
    let line = lines.get(start_line).copied().unwrap_or("");
    let trimmed = line.trim();

    // Skip empty lines and comments
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return Err(start_line + 1);
    }

    // Check for section header
    if !trimmed.starts_with("---") {
        // Not a section header, skip line
        return Err(start_line + 1);
    }

    // Parse section header
    let section_type = match parse_section_header(trimmed, start_line, diagnostics) {
        Some(t) => t,
        None => return Err(start_line + 1),
    };

    // Find section content
    let content_start = start_line + 1;
    let (content, content_end) = extract_section_content(lines, content_start, section_type);

    // Parse section content with error isolation
    let content_result = parse_section_content(&content, content_start, section_type, diagnostics);

    let section = Section {
        section_type,
        content: content_result,
        inline_options: Default::default(),
        raw_content: content.join("\n"),
        start_line,
        end_line: content_end,
    };

    Ok((section, content_end + 1))
}

/// Parse section header like "--- ENDPOINT ---"
fn parse_section_header(
    line: &str,
    line_num: usize,
    diagnostics: &mut DiagnosticCollection,
) -> Option<SectionType> {
    // Remove --- delimiters
    let without_delimiters = line.trim_start_matches('-').trim_end_matches('-').trim();

    // Extract section name and inline options
    let parts: Vec<&str> = without_delimiters.splitn(2, ' ').collect();
    let section_name = parts[0].trim();

    // Parse section type
    let section_type = match section_name.to_uppercase().as_str() {
        "ADDRESS" => SectionType::Address,
        "ENDPOINT" => SectionType::Endpoint,
        "REQUEST" => SectionType::Request,
        "RESPONSE" => SectionType::Response,
        "ERROR" => SectionType::Error,
        "EXTRACT" => SectionType::Extract,
        "ASSERTS" => SectionType::Asserts,
        "REQUEST_HEADERS" => SectionType::RequestHeaders,
        "TLS" => SectionType::Tls,
        "PROTO" => SectionType::Proto,
        "OPTIONS" => SectionType::Options,
        _ => {
            diagnostics.warning(
                DiagnosticCode::UnknownSectionType,
                format!("Unknown section type: {}", section_name),
                Range::at_line(line_num),
            );
            return None;
        }
    };

    // Parse inline options if present
    if parts.len() > 1 {
        parse_inline_options(parts[1], line_num, diagnostics);
    }

    Some(section_type)
}

/// Extract content lines for a section
fn extract_section_content(
    lines: &[&str],
    start: usize,
    _section_type: SectionType,
) -> (Vec<String>, usize) {
    let mut content = Vec::new();
    let mut end_line = start;

    for (i, line) in lines.iter().enumerate().skip(start) {
        let trimmed = line.trim();

        // Check for next section header
        if trimmed.starts_with("---") && trimmed.ends_with("---") {
            break;
        }

        content.push(line.to_string());
        end_line = i;
    }

    (content, end_line)
}

/// Parse section content based on type
fn parse_section_content(
    content: &[String],
    start_line: usize,
    section_type: SectionType,
    diagnostics: &mut DiagnosticCollection,
) -> SectionContent {
    let content_str = content.join("\n");

    match section_type {
        SectionType::Address | SectionType::Endpoint => {
            SectionContent::Single(content_str.trim().to_string())
        }
        SectionType::Request | SectionType::Response | SectionType::Error => {
            if content_str.trim().is_empty() {
                SectionContent::Empty
            } else {
                // Try to parse as JSON, but don't fail - just add diagnostic
                match serde_json::from_str::<serde_json::Value>(&content_str) {
                    Ok(value) => SectionContent::Json(value),
                    Err(e) => {
                        // Add error but continue parsing
                        diagnostics.error(
                            DiagnosticCode::JsonParseError,
                            format!("Failed to parse JSON: {}", e),
                            Range::at_line(start_line),
                        );
                        // Return as-is to allow further processing
                        SectionContent::Json(serde_json::Value::String(content_str))
                    }
                }
            }
        }
        SectionType::Extract => {
            // Parse extract variables
            let mut extractions = std::collections::HashMap::new();
            for (i, line) in content.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                if let Some(eq_pos) = trimmed.find('=') {
                    let name = trimmed[..eq_pos].trim().to_string();
                    let query = trimmed[eq_pos + 1..].trim().to_string();
                    extractions.insert(name, query);
                } else {
                    diagnostics.warning(
                        DiagnosticCode::InvalidSyntax,
                        "Invalid EXTRACT syntax, expected: name = query",
                        Range::at_line(start_line + i),
                    );
                }
            }
            SectionContent::Extract(extractions)
        }
        SectionType::Asserts => {
            // Collect assertion lines
            let assertions: Vec<String> = content
                .iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            SectionContent::Assertions(assertions)
        }
        SectionType::RequestHeaders
        | SectionType::Tls
        | SectionType::Proto
        | SectionType::Options => {
            // Parse key-value pairs
            let mut key_values = std::collections::HashMap::new();
            for (i, line) in content.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                if let Some(colon_pos) = trimmed.find(':') {
                    let key = trimmed[..colon_pos].trim().to_string();
                    let value = trimmed[colon_pos + 1..].trim().to_string();
                    key_values.insert(key, value);
                } else {
                    diagnostics.warning(
                        DiagnosticCode::InvalidSyntax,
                        "Invalid key-value syntax, expected: key: value",
                        Range::at_line(start_line + i),
                    );
                }
            }
            SectionContent::KeyValues(key_values)
        }
    }
}

/// Parse inline options like "with_asserts=true"
fn parse_inline_options(
    options_str: &str,
    line_num: usize,
    diagnostics: &mut DiagnosticCollection,
) {
    // Parse options like: with_asserts=true unordered_arrays=true
    for option in options_str.split_whitespace() {
        if let Some(eq_pos) = option.find('=') {
            let key = &option[..eq_pos];
            let value = &option[eq_pos + 1..];

            match key {
                "with_asserts" | "unordered_arrays" | "partial" => {
                    // Valid boolean options
                    if value != "true" && value != "false" {
                        diagnostics.warning(
                            DiagnosticCode::InvalidFieldValue,
                            format!("Invalid boolean value for {}: {}", key, value),
                            Range::at_line(line_num),
                        );
                    }
                }
                "tolerance" => {
                    // Numeric option
                    if value.parse::<f64>().is_err() {
                        diagnostics.warning(
                            DiagnosticCode::InvalidFieldValue,
                            format!("Invalid numeric value for {}: {}", key, value),
                            Range::at_line(line_num),
                        );
                    }
                }
                _ => {
                    diagnostics.hint(
                        DiagnosticCode::InvalidFieldValue,
                        format!("Unknown inline option: {}", key),
                        Range::at_line(line_num),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_with_recovery_valid_file() {
        let content = r#"--- ENDPOINT ---
service/Method

--- REQUEST ---
{"key": "value"}

--- RESPONSE ---
{"result": "ok"}
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        assert_eq!(result.recovered_sections, 3);
        assert_eq!(result.failed_sections, 0);
        assert!(!result.document.sections.is_empty());
    }

    #[test]
    fn test_parse_with_recovery_invalid_json() {
        let content = r#"--- ENDPOINT ---
service/Method

--- REQUEST ---
{"key": "value"

--- RESPONSE ---
{"result": "ok"}
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        // Should recover and continue parsing
        assert_eq!(result.recovered_sections, 3);
        assert_eq!(result.failed_sections, 0);
        // Should have diagnostic for invalid JSON
        assert!(result.diagnostics.has_errors());
    }

    #[test]
    fn test_parse_with_recovery_multiple_errors() {
        let content = r#"--- ENDPOINT ---
service/Method

--- REQUEST ---
{invalid json

--- RESPONSE ---
{also invalid

--- EXTRACT ---
var = .field
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        // Should recover all sections
        assert_eq!(result.recovered_sections, 4);
        // Should have multiple diagnostics
        assert!(result.diagnostics.diagnostics.len() >= 2);
    }

    #[test]
    fn test_parse_with_recovery_unknown_section() {
        let content = r#"--- ENDPOINT ---
service/Method

--- UNKNOWN_SECTION ---
content

--- RESPONSE ---
{"ok": true}
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        // Should skip unknown section
        assert!(result.diagnostics.has_warnings());
    }

    #[test]
    fn test_parse_with_recovery_invalid_extract() {
        let content = r#"--- EXTRACT ---
valid = .field
invalid line without equals
another = .field2
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        // Should parse valid extracts and warn about invalid
        assert!(result.diagnostics.has_warnings());
    }
}
