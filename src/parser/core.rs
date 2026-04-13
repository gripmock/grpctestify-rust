// GCTF file parser - converts .gctf text to AST
// Handles section extraction, comment removal, and inline option parsing

use super::ast::*;
use super::content_parser;
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

/// Parse .gctf content from string (for LSP/editor use).
/// Documents are determined implicitly: REQUEST after RESPONSE/ERROR/ASSERTS,
/// or ENDPOINT/ADDRESS starts a new document.
pub fn parse_gctf_from_str(content: &str, file_path: &str) -> Result<GctfDocument> {
    let source_lines: Vec<&str> = content.lines().collect();
    let (all_sections, _) = parse_sections(&source_lines)?;

    // Split sections into documents based on implicit boundaries
    let documents = split_into_documents(&all_sections);

    if documents.is_empty() {
        // Return empty single document for backward compatibility
        let mut document = GctfDocument::new(file_path.to_string());
        document.metadata.source = Some(content.to_string());
        return Ok(document);
    }

    // Build chain in reverse order
    let mut head: Option<GctfDocument> = None;

    for doc_sections in documents.into_iter().rev() {
        let mut document = GctfDocument::new(file_path.to_string());
        document.metadata.source = Some(extract_doc_source(&doc_sections, content));
        document.sections = doc_sections;
        document.next_document = head.map(Box::new);
        head = Some(document);
    }

    head.ok_or_else(|| anyhow::anyhow!("No documents parsed"))
}

/// Split sections into documents based on implicit boundaries.
///
/// Boundary = ENDPOINT with meaningful content before it.
///
/// "Preamble" sections (ADDRESS, TLS, PROTO, OPTIONS, REQUEST_HEADERS)
/// that appear immediately before ENDPOINT are moved to the new document.
/// They configure the next request, not the previous one.
///
/// The only thing propagated between documents is EXTRACT variables —
/// they become part of the execution context, not the AST.
fn split_into_documents(sections: &[Section]) -> Vec<Vec<Section>> {
    if sections.is_empty() {
        return Vec::new();
    }

    let mut docs: Vec<Vec<Section>> = Vec::new();
    let mut current: Vec<Section> = Vec::new();

    // Sections that are "preambles" — they belong to the next document
    let is_preamble = |t: &SectionType| {
        matches!(
            t,
            SectionType::Address
                | SectionType::Tls
                | SectionType::Proto
                | SectionType::Options
                | SectionType::RequestHeaders
        )
    };

    for section in sections {
        if section.section_type == SectionType::Endpoint {
            // Check if current has meaningful content (request/response cycle completed)
            let has_content = current.iter().any(|s| {
                matches!(
                    s.section_type,
                    SectionType::Request
                        | SectionType::Response
                        | SectionType::Error
                        | SectionType::Asserts
                        | SectionType::Extract
                )
            });

            if has_content {
                // Before splitting: move trailing preamble sections to new document
                let mut preamble: Vec<Section> = Vec::new();
                while let Some(last) = current.last() {
                    if is_preamble(&last.section_type) {
                        preamble.insert(0, current.pop().unwrap());
                    } else {
                        break;
                    }
                }

                docs.push(std::mem::take(&mut current));
                current = preamble;
            }
            // If no content yet (only preamble), preamble stays with ENDPOINT — no split
        }

        current.push(section.clone());
    }

    if !current.is_empty() {
        docs.push(current);
    }

    docs
}

/// Extract source lines for a document from the original content.
fn extract_doc_source(sections: &[Section], original: &str) -> String {
    if sections.is_empty() {
        return String::new();
    }
    let start = sections.first().unwrap().start_line;
    let end = sections.last().unwrap().end_line;
    original
        .lines()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<Vec<_>>()
        .join("\n")
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

/// Parse .gctf and return AST + diagnostics useful for inspect/debug.
/// Supports multiple documents with implicit boundaries (ENDPOINT after terminal section).
pub fn parse_gctf_with_diagnostics(file_path: &Path) -> Result<(GctfDocument, ParseDiagnostics)> {
    let total_start = Instant::now();

    let read_start = Instant::now();
    let source = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
    let read_ms = read_start.elapsed().as_secs_f64() * 1000.0;

    let source_lines: Vec<&str> = source.lines().collect();

    let parse_sections_start = Instant::now();
    let (sections, section_headers) = parse_sections(&source_lines)?;
    let parse_sections_ms = parse_sections_start.elapsed().as_secs_f64() * 1000.0;

    // Split into documents using implicit boundaries
    let documents = split_into_documents(&sections);

    let build_start = Instant::now();
    // Build chain — return head document
    let mut head: Option<GctfDocument> = None;
    for doc_sections in documents.into_iter().rev() {
        let mut document = GctfDocument::new(file_path.display().to_string());
        document.metadata.source = Some(source.clone());
        document.sections = doc_sections;
        document.next_document = head.map(Box::new);
        head = Some(document);
    }

    let document = head.unwrap_or_else(|| {
        let mut doc = GctfDocument::new(file_path.display().to_string());
        doc.metadata.source = Some(source.clone());
        doc
    });
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;
    let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    let mut section_counts: HashMap<String, usize> = HashMap::new();
    for d in document.iter_chain() {
        for section in &d.sections {
            *section_counts
                .entry(section.section_type.as_str().to_string())
                .or_insert(0) += 1;
        }
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
            build_document_ms: build_ms,
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
                let section = content_parser::build_section(
                    section_type,
                    start_line,
                    line_idx,
                    &content,
                    options,
                )?;
                sections.push(section);
            }

            section_headers += 1;

            // Start new section
            let section_name = captures.get(1).unwrap().as_str();
            let inline_options_str = captures.get(2).map(|m| m.as_str());

            if let Some(section_type) = SectionType::from_keyword(section_name) {
                let inline_options = if section_type.supports_inline_options() {
                    if let Some(opts_str) = inline_options_str {
                        content_parser::parse_inline_options(opts_str)?
                    } else {
                        InlineOptions::default()
                    }
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
        let section =
            content_parser::build_section(section_type, start_line, end_line, &content, options)?;
        sections.push(section);
    }

    Ok((sections, section_headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sections_basic() {
        let lines = vec![
            "--- ENDPOINT ---",
            "test.Service/Method",
            "",
            "--- REQUEST ---",
            "{}",
            "",
            "--- RESPONSE ---",
            "{}",
        ];
        let (sections, count) = parse_sections(&lines).unwrap();
        assert_eq!(count, 3);
        assert_eq!(sections.len(), 3);
    }

    #[test]
    fn test_section_header_regex() {
        assert!(SECTION_HEADER_REGEX.is_match("--- ENDPOINT ---"));
        assert!(SECTION_HEADER_REGEX.is_match("--- REQUEST ---"));
        assert!(SECTION_HEADER_REGEX.is_match("--- RESPONSE partial=true ---"));
        assert!(!SECTION_HEADER_REGEX.is_match("not a section header"));
    }
}
