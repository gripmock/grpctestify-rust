// Error recovery parser for GCTF files
// Parses as much as possible and collects all errors

use crate::assertions::strip_assertion_comments;
use crate::ast::{DocumentMetadata, FileMeta, GctfDocument, Section, SectionContent, SectionType};
use crate::gctf_tokenizer;
use apif_diagnostics::{DiagnosticCode, DiagnosticCollection, Range};
use std::path::Path;

/// Result of error recovery parsing
pub struct ErrorRecoveryResult {
    pub document: GctfDocument,
    pub diagnostics: DiagnosticCollection,
    pub recovered_sections: usize,
    pub failed_sections: usize,
}

/// Parse GCTF file with error recovery.
/// Supports multiple documents via document chain.
pub fn parse_with_recovery(file_path: &Path) -> ErrorRecoveryResult {
    let content = std::fs::read_to_string(file_path).unwrap_or_default();
    parse_content_with_recovery(&content, file_path.to_string_lossy().as_ref())
}

/// Parse GCTF content string with error recovery.
/// Documents are determined implicitly: REQUEST after RESPONSE/ERROR/ASSERTS,
/// or ENDPOINT/ADDRESS starts a new document.
pub fn parse_content_with_recovery(content: &str, file_path: &str) -> ErrorRecoveryResult {
    let single = parse_single_with_recovery(content, file_path);

    // Split by implicit boundaries
    let docs = crate::split_sections_by_boundary(&single.document.sections);

    if docs.len() <= 1 {
        return single;
    }

    // Link in reverse
    let mut head: Option<GctfDocument> = None;
    let total_recovered = single.recovered_sections;
    let total_failed = single.failed_sections;

    for doc_sections in docs.into_iter().rev() {
        let mut doc = build_doc_from_sections(&doc_sections, file_path);
        doc.next_document = head.map(Box::new);
        head = Some(doc);
    }

    ErrorRecoveryResult {
        document: head.unwrap_or(single.document),
        diagnostics: single.diagnostics,
        recovered_sections: total_recovered,
        failed_sections: total_failed,
    }
}

fn build_doc_from_sections(sections: &[Section], file_path: &str) -> GctfDocument {
    GctfDocument {
        file_path: file_path.to_string(),
        sections: sections.to_vec(),
        metadata: DocumentMetadata {
            source: None,
            mtime: None,
            parsed_at: 0,
            ..Default::default()
        },
        next_document: None,
    }
}

/// Parse a single document (no `--- NEW ---` splitting)
fn parse_single_with_recovery(content: &str, file_path: &str) -> ErrorRecoveryResult {
    let mut diagnostics = DiagnosticCollection::new();
    let mut sections = Vec::new();
    let mut recovered_sections = 0;
    let failed_sections = 0;

    let lines: Vec<&str> = content.lines().collect();
    let mut current_line = 0;

    let mut pending_attributes: Vec<crate::ast::GctfAttribute> = Vec::new();

    while current_line < lines.len() {
        match parse_section(
            &lines,
            current_line,
            &mut diagnostics,
            &mut pending_attributes,
        ) {
            Ok((section, end_line)) => {
                sections.push(section);
                recovered_sections += 1;
                current_line = end_line;
            }
            Err(end_line) => {
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
            ..Default::default()
        },
        next_document: None,
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
    pending_attributes: &mut Vec<crate::ast::GctfAttribute>,
) -> Result<(Section, usize), usize> {
    let line = lines.get(start_line).copied().unwrap_or("");
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with("//") {
        return Err(start_line + 1);
    }

    if trimmed.starts_with("#[") && trimmed.ends_with(']') {
        let inner = &trimmed[2..trimmed.len() - 1];
        if let Some(attr) = crate::content_parser::parse_attribute(inner) {
            pending_attributes.push(attr);
        }
        return Err(start_line + 1);
    }

    if !trimmed.starts_with("---") {
        return Err(start_line + 1);
    }

    let (section_type, inline_options) =
        match parse_section_header(trimmed, start_line, diagnostics) {
            Some(t) => t,
            None => return Err(start_line + 1),
        };

    let content_start = start_line + 1;
    let (content, content_end) = extract_section_content(lines, content_start, section_type);

    let content_result = parse_section_content(&content, content_start, section_type, diagnostics);

    let section = Section {
        section_type,
        content: content_result,
        inline_options,
        raw_content: content.join("\n"),
        start_line,
        end_line: content_end,
        attributes: std::mem::take(pending_attributes),
    };

    Ok((section, content_end + 1))
}

/// Parse section header like "--- ENDPOINT ---"
fn parse_section_header(
    line: &str,
    line_num: usize,
    diagnostics: &mut DiagnosticCollection,
) -> Option<(SectionType, crate::ast::InlineOptions)> {
    // Remove --- delimiters
    let without_delimiters = line.trim_start_matches('-').trim_end_matches('-').trim();

    // Extract section name and inline options
    let (section_name, inline_opts_str) = without_delimiters
        .split_once(' ')
        .map_or((without_delimiters, ""), |(name, opts)| (name, opts));
    let section_name = section_name.trim();
    let inline_opts_str = inline_opts_str.trim();

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
        "HEADERS" => {
            diagnostics.warning(
                DiagnosticCode::DeprecatedSymbol,
                "HEADERS is deprecated, use REQUEST_HEADERS".to_string(),
                Range::at_line(line_num),
            );
            SectionType::RequestHeaders
        }
        "TLS" => SectionType::Tls,
        "PROTO" => SectionType::Proto,
        "OPTIONS" => SectionType::Options,
        "META" => SectionType::Meta,
        "BENCH" => SectionType::Bench,
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
    let has_opts = !inline_opts_str.is_empty();
    let inline_options = if has_opts && section_type.supports_inline_options() {
        match crate::content_parser::parse_inline_options(inline_opts_str) {
            Ok(opts) => opts,
            Err(_) => {
                parse_inline_options_diagnostic(inline_opts_str, line_num, diagnostics);
                Default::default()
            }
        }
    } else {
        if has_opts {
            parse_inline_options_diagnostic(inline_opts_str, line_num, diagnostics);
        }
        Default::default()
    };

    Some((section_type, inline_options))
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

        // Skip attribute lines (they belong to the next section, not current)
        if trimmed.starts_with("#[") && trimmed.ends_with(']') {
            continue;
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
                // Try to parse as JSON5 (with comments), but don't fail - just add diagnostic
                match crate::json_mod::from_str(&content_str) {
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
                .filter_map(|line| strip_assertion_comments(line))
                .collect();
            SectionContent::Assertions(assertions)
        }
        SectionType::RequestHeaders
        | SectionType::Tls
        | SectionType::Proto
        | SectionType::Options
        | SectionType::Bench => {
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
        SectionType::Meta => {
            // Use tokenizer to strip GCTF comment lines before parsing YAML
            let raw = content.join("\n");
            let tokens = gctf_tokenizer::tokenize_gctf(&raw);
            let yaml_lines: Vec<String> = content
                .iter()
                .zip(tokens.iter())
                .filter(|(_, t)| !matches!(t.kind, gctf_tokenizer::GctfTokenKind::Comment(_)))
                .map(|(l, _)| l.clone())
                .collect();
            let cleaned = yaml_lines.join("\n");
            let meta = serde_yaml_ng::from_str::<FileMeta>(&cleaned).unwrap_or_default();
            SectionContent::Meta(meta)
        }
    }
}

/// Parse inline options like "with_asserts=true"
fn parse_inline_options_diagnostic(
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
                    if value != "true" && value != "false" {
                        diagnostics.warning(
                            DiagnosticCode::InvalidFieldValue,
                            format!("Invalid boolean value for {}: {}", key, value),
                            Range::at_line(line_num),
                        );
                    }
                }
                "tolerance" => {
                    if value.parse::<f64>().is_err() {
                        diagnostics.warning(
                            DiagnosticCode::InvalidFieldValue,
                            format!("Invalid numeric value for {}: {}", key, value),
                            Range::at_line(line_num),
                        );
                    }
                }
                "redact" => {}
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

    #[test]
    fn test_parse_with_recovery_asserts_double_slash_comments() {
        let content = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{"service": "examples.health.watch"}

--- ASSERTS ---
// Watch delay in stubs.yaml is 10ms.
// Delay applies before the first message in the scope.
@scope.message_count() == 2
@elapsed_ms() >= 10
@total_elapsed_ms() >= 10
"#;

        let result = parse_content_with_recovery(content, "test.gctf");
        let asserts = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Asserts)
            .expect("ASSERTS section should be parsed");

        if let SectionContent::Assertions(lines) = &asserts.content {
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0], "@scope.message_count() == 2");
            assert_eq!(lines[1], "@elapsed_ms() >= 10");
            assert_eq!(lines[2], "@total_elapsed_ms() >= 10");
        } else {
            panic!("expected assertions content");
        }
    }

    #[test]
    fn test_parse_with_recovery_asserts_inline_comments() {
        let content = r#"--- ENDPOINT ---
grpc.health.v1.Health/Watch

--- REQUEST ---
{"service": "examples.health.watch"}

--- ASSERTS ---
@scope.message_count() == 2 // exactly two updates expected
@elapsed_ms() >= 10 # startup delay should be applied
@regex(.note, "^https://example.com")
"#;

        let result = parse_content_with_recovery(content, "test.gctf");
        let asserts = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Asserts)
            .expect("ASSERTS section should be parsed");

        if let SectionContent::Assertions(lines) = &asserts.content {
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0], "@scope.message_count() == 2");
            assert_eq!(lines[1], "@elapsed_ms() >= 10");
            assert_eq!(lines[2], "@regex(.note, \"^https://example.com\")");
        } else {
            panic!("expected assertions content");
        }
    }

    #[test]
    fn test_parse_with_recovery_headers_deprecated() {
        let content = r#"--- ENDPOINT ---
svc/Method

--- HEADERS ---
content-type: application/grpc

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
        // Should still parse REQUEST and RESPONSE
        assert_eq!(result.recovered_sections, 4);
    }

    #[test]
    fn test_parse_with_recovery_tls_section_key_values() {
        let content = r#"--- TLS ---
enabled: true
cert_path: /path/to/cert

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 4);
        let tls = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Tls)
            .expect("TLS section should be parsed");
        if let SectionContent::KeyValues(kvs) = &tls.content {
            assert_eq!(kvs.get("enabled"), Some(&"true".to_string()));
            assert_eq!(kvs.get("cert_path"), Some(&"/path/to/cert".to_string()));
        } else {
            panic!("expected key-values content");
        }
    }

    #[test]
    fn test_parse_with_recovery_options_section() {
        let content = r#"--- OPTIONS ---
timeout: 5000
retries: 3

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 4);
        let opts = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Options)
            .expect("OPTIONS section should be parsed");
        if let SectionContent::KeyValues(kvs) = &opts.content {
            assert_eq!(kvs.get("timeout"), Some(&"5000".to_string()));
            assert_eq!(kvs.get("retries"), Some(&"3".to_string()));
        } else {
            panic!("expected key-values content");
        }
    }

    #[test]
    fn test_parse_with_recovery_proto_section() {
        let content = r#"--- PROTO ---
protos: ["service.proto"]
import_dirs: ["/protos"]

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 4);
    }

    #[test]
    fn test_parse_with_recovery_empty_response() {
        let content = r#"--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---

"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 3);
        let response = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Response)
            .expect("RESPONSE section should exist");
        assert!(matches!(response.content, SectionContent::Empty));
    }

    #[test]
    fn test_parse_with_recovery_non_section_header_lines() {
        let content = r#"some random line
more text
--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        // Non-section-header lines should be skipped
        assert_eq!(result.recovered_sections, 3);
    }

    #[test]
    fn test_parse_with_recovery_comment_lines() {
        let content = r#"# This is a comment
// Another comment

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 3);
    }

    #[test]
    fn test_parse_with_recovery_inline_options_invalid_boolean() {
        let content = r#"--- ENDPOINT with_asserts=maybe ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
        assert_eq!(result.recovered_sections, 3);
    }

    #[test]
    fn test_parse_with_recovery_inline_options_invalid_numeric() {
        let content = r#"--- ENDPOINT tolerance=abc ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
    }

    #[test]
    fn test_parse_with_recovery_inline_options_unknown() {
        let content = r#"--- ENDPOINT unknown_option=value ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        // Unknown options produce hints, not warnings
        assert_eq!(result.recovered_sections, 3);
    }

    #[test]
    fn test_parse_with_recovery_inline_options_valid() {
        let content = r#"--- ENDPOINT with_asserts=true unordered_arrays=true partial=false tolerance=0.05 ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 3);
    }

    #[test]
    fn test_parse_with_recovery_request_headers_section() {
        let content = r#"--- ENDPOINT ---
svc/Method

--- REQUEST_HEADERS ---
authorization: Bearer token
x-custom: value

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 4);
        let headers = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::RequestHeaders)
            .expect("REQUEST_HEADERS section should be parsed");
        if let SectionContent::KeyValues(kvs) = &headers.content {
            assert_eq!(kvs.get("authorization"), Some(&"Bearer token".to_string()));
            assert_eq!(kvs.get("x-custom"), Some(&"value".to_string()));
        } else {
            panic!("expected key-values content");
        }
    }

    #[test]
    fn test_parse_with_recovery_invalid_key_value_syntax() {
        let content = r#"--- TLS ---
enabled: true
invalid line without colon

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
        assert_eq!(result.recovered_sections, 4);
    }
}
