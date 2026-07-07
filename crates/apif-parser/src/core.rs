// GCTF file parser - converts .gctf text to AST
// Handles section extraction, comment removal, and inline option parsing

use apif_ast::*;
use crate::content_parser::{self, parse_attribute};
use apif_ast::gctf_tokenizer::{GctfTokenKind, tokenize_gctf};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Parse a .gctf file into an AST
pub fn parse_gctf(file_path: &Path) -> Result<GctfDocument> {
    let (document, _) = parse_gctf_with_diagnostics(file_path)?;
    Ok(document)
}

/// Parse .gctf content from string (for LSP/editor use).
/// Documents are determined implicitly: REQUEST after RESPONSE/ERROR/ASSERTS,
/// or ENDPOINT/ADDRESS starts a new document.
pub fn parse_gctf_from_str(content: &str, file_path: &str) -> Result<GctfDocument> {
    let (all_sections, _) = parse_sections_from_str(content)?;
    let source_lines: Vec<&str> = content.lines().collect();

    // Split sections into documents based on implicit boundaries
    let documents = crate::document_splitter::split_sections_by_boundary_owned(all_sections);

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
        document.metadata.source =
            Some(extract_doc_source_from_lines(&doc_sections, &source_lines));
        document.sections = doc_sections;
        document.next_document = head.map(Box::new);
        head = Some(document);
    }

    head.ok_or_else(|| anyhow::anyhow!("No documents parsed"))
}

/// Split sections into documents based on implicit boundaries.
///
/// Extract source lines for a document from the original content.
fn extract_doc_source_from_lines(sections: &[Section], lines: &[&str]) -> String {
    if sections.is_empty() {
        return String::new();
    }

    let (start, end) = match (sections.first(), sections.last()) {
        (Some(first), Some(last)) => (first.start_line, last.end_line),
        _ => return String::new(),
    };
    lines.get(start..end).unwrap_or(&[]).join("\n")
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

    let parse_sections_start = Instant::now();
    let (sections, section_headers) = parse_sections_from_str(&source)?;
    let parse_sections_ms = parse_sections_start.elapsed().as_secs_f64() * 1000.0;

    // Split into documents using implicit boundaries
    let documents = crate::document_splitter::split_sections_by_boundary_owned(sections);

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
        total_lines: source.lines().count(),
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

#[allow(clippy::type_complexity)]
fn parse_sections_from_str(source: &str) -> Result<(Vec<Section>, usize)> {
    let tokens = tokenize_gctf(source);
    let mut sections = Vec::new();
    let mut section_headers = 0;
    let mut current_section: Option<(
        SectionType,
        usize,
        Vec<String>,
        InlineOptions,
        Vec<GctfAttribute>,
    )> = None;
    let mut pending_attributes: Vec<GctfAttribute> = Vec::new();

    for token in tokens {
        match token.kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                if let Some((section_type, start_line, content, options, raw_attrs)) =
                    current_section.take()
                {
                    let section = content_parser::build_section(
                        section_type,
                        start_line,
                        token.line,
                        &content,
                        options,
                        raw_attrs,
                    )?;
                    sections.push(section);
                }

                section_headers += 1;

                if let Some(section_type) = SectionType::from_keyword(&name) {
                    let inline_options =
                        if section_type.supports_inline_options() && !raw_options.is_empty() {
                            content_parser::parse_inline_options(&raw_options)?
                        } else {
                            InlineOptions::default()
                        };
                    current_section = Some((
                        section_type,
                        token.line,
                        Vec::new(),
                        inline_options,
                        std::mem::take(&mut pending_attributes),
                    ));
                } else {
                    return Err(anyhow::anyhow!("Unknown section type: {}", name));
                }
            }
            GctfTokenKind::AttributeBlock(attr_content) => {
                if let Some(attr) = parse_attribute(&attr_content) {
                    pending_attributes.push(attr);
                }
            }
            GctfTokenKind::Comment(text) | GctfTokenKind::Content(text) => {
                if let Some((_, _, ref mut content, _, _)) = current_section {
                    content.push(text);
                }
            }
            GctfTokenKind::Blank => {
                if let Some((_, _, ref mut content, _, _)) = current_section {
                    content.push(String::new());
                }
            }
        }
    }

    if let Some((section_type, start_line, content, options, raw_attrs)) = current_section {
        let end_line = source.lines().count();
        let section = content_parser::build_section(
            section_type,
            start_line,
            end_line,
            &content,
            options,
            raw_attrs,
        )?;
        sections.push(section);
    }
    Ok((sections, section_headers))
}

/// Format/serialize a GCTF document to string
pub fn serialize_gctf(doc: &GctfDocument) -> String {
    use std::fmt::Write;
    let mut output = String::new();

    let sections = sort_sections_for_fmt(&doc.sections);

    for section in &sections {
        for attr in &section.attributes {
            let _ = writeln!(output, "{}", attr.format_directive());
        }

        let _ = write!(output, "--- {} ---", section.section_type.as_str());
        output.push('\n');

        match &section.content {
            SectionContent::Single(s) => {
                let _ = writeln!(output, "{}", s.trim());
            }
            SectionContent::Json(val) => {
                if let Ok(pretty) = serde_json::to_string_pretty(val) {
                    let _ = writeln!(output, "{}", pretty);
                } else {
                    let raw = section.raw_content.trim();
                    let _ = writeln!(output, "{}", raw);
                }
            }
            SectionContent::JsonLines(lines) => {
                for val in lines {
                    if let Ok(compact) = serde_json::to_string(val) {
                        let _ = writeln!(output, "{}", compact);
                    }
                }
            }
            SectionContent::KeyValues(kv) => {
                let mut sorted: Vec<_> = kv.iter().collect();
                if section.section_type == SectionType::Bench {
                    sorted.sort_by(|a, b| {
                        bench_key_rank(a.0)
                            .cmp(&bench_key_rank(b.0))
                            .then_with(|| a.0.cmp(b.0))
                    });
                } else {
                    sorted.sort_by(|a, b| a.0.cmp(b.0));
                }
                for (k, v) in sorted {
                    let _ = writeln!(output, "{}: {}", k, v);
                }
            }
            SectionContent::Assertions(lines) => {
                for line in lines {
                    let _ = writeln!(output, "{}", line.trim());
                }
            }
            SectionContent::Empty => {}
            SectionContent::Extract(vars) => {
                let mut sorted: Vec<_> = vars.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in sorted {
                    let _ = writeln!(output, "{}: {}", k, v);
                }
            }
            SectionContent::Meta(meta) => {
                if let Ok(yaml) = serde_yaml_ng::to_string(meta) {
                    output.push_str(yaml.trim_end());
                }
            }
        }
        output.push('\n');
    }

    output.trim_end().to_string() + "\n"
}

fn sort_sections_for_fmt(sections: &[Section]) -> Vec<Section> {
    if sections.len() <= 1 {
        return sections.to_vec();
    }

    let first_body_idx = sections
        .iter()
        .position(|s| s.section_type.preamble_rank().is_none())
        .unwrap_or(sections.len());

    let mut preamble: Vec<&Section> = sections[..first_body_idx].iter().collect();
    preamble.sort_by_key(|s| s.section_type.preamble_rank().unwrap());

    let mut result = Vec::with_capacity(sections.len());
    for s in &preamble {
        result.push((*s).clone());
    }
    for s in &sections[first_body_idx..] {
        result.push((*s).clone());
    }
    result
}

fn bench_key_rank(key: &str) -> usize {
    let canonical_order = [
        "mode",
        "profile",
        "name",
        "concurrency",
        "requests",
        "duration",
        "max_duration",
        "ramp_up",
        "warmup",
        "warmup_mode",
        "cool_down",
        "max_rps",
        "load_schedule",
        "load_start",
        "load_step",
        "load_end",
        "load_step_duration",
        "load_max_duration",
        "load_midpoint",
        "load_amplitude",
        "load_frequency",
        "load_spike_target",
        "load_spike_after",
        "load_spike_duration",
        "load_profile",
        "progress_interval",
        "connections",
        "connect_timeout",
        "keepalive",
        "cpus",
        "assert_mode",
        "no_assert",
        "sample_rate",
        "duration_stop",
        "cache",
        "cache_ttl",
        "skip_first",
        "count_errors_in_latency",
        "latency_percentiles",
        "sources",
    ];

    if let Some((idx, _)) = canonical_order.iter().enumerate().find(|(_, k)| **k == key) {
        return idx;
    }
    if key.starts_with("thresholds.") || key == "thresholds" {
        return canonical_order.len();
    }
    usize::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_sections_basic() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
";
        let (sections, count) = parse_sections_from_str(input).unwrap();
        assert_eq!(count, 3);
        assert_eq!(sections.len(), 3);
    }

    #[test]
    fn test_section_header_tokenizer() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE partial=true ---
{}
";
        let (sections, count) = parse_sections_from_str(input).unwrap();
        assert_eq!(count, 3);
        assert_eq!(sections.len(), 3);

        let resp = sections
            .iter()
            .find(|s| s.section_type == SectionType::Response)
            .unwrap();
        assert!(resp.inline_options.partial);
    }

    #[test]
    fn test_parse_multi_document() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- ENDPOINT ---
test.Service/Method2

--- REQUEST ---
{\"a\": 1}

--- RESPONSE ---
{\"b\": 2}
";
        let doc = parse_gctf_from_str(input, "test.gctf").unwrap();
        assert_eq!(doc.document_count(), 2);

        let first_endpoint = doc.get_endpoint().unwrap();
        assert_eq!(first_endpoint, "test.Service/Method");

        let second = doc.get_document(1).unwrap();
        assert_eq!(second.get_endpoint().unwrap(), "test.Service/Method2");
    }

    #[test]
    fn test_parse_empty_content() {
        let doc = parse_gctf_from_str("", "test.gctf").unwrap();
        assert!(doc.sections.is_empty());
    }

    #[test]
    fn test_parse_all_section_types() {
        let input = "\
--- ADDRESS ---
localhost:50051

--- ENDPOINT ---
test.Service/Method

--- TLS ---
ca_cert: /path/ca.pem

--- PROTO ---
files: service.proto

--- OPTIONS ---
timeout: 10

--- REQUEST_HEADERS ---
Authorization: Bearer token

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- ASSERTS ---
.x == 1

--- EXTRACT ---
total = .response.total
";
        let (sections, count) = parse_sections_from_str(input).unwrap();
        assert_eq!(count, 10);

        let types: Vec<SectionType> = sections.iter().map(|s| s.section_type).collect();
        assert_eq!(types[0], SectionType::Address);
        assert_eq!(types[1], SectionType::Endpoint);
        assert_eq!(types[2], SectionType::Tls);
        assert_eq!(types[3], SectionType::Proto);
        assert_eq!(types[4], SectionType::Options);
        assert_eq!(types[5], SectionType::RequestHeaders);
        assert_eq!(types[6], SectionType::Request);
        assert_eq!(types[7], SectionType::Response);
        assert_eq!(types[8], SectionType::Asserts);
        assert_eq!(types[9], SectionType::Extract);
    }

    #[test]
    fn test_parse_unknown_section_type() {
        let input = "--- UNKNOWN ---\nhello\n";
        let result = parse_sections_from_str(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown section type")
        );
    }

    #[test]
    fn test_parse_preserves_comments_in_content() {
        let input = "\
--- RESPONSE ---
// This is a comment
{\"status\": \"OK\"}
# Another comment
";
        let (sections, _) = parse_sections_from_str(input).unwrap();
        let resp = sections
            .into_iter()
            .find(|s| s.section_type == SectionType::Response)
            .unwrap();
        assert!(resp.raw_content.contains("// This is a comment"));
        assert!(resp.raw_content.contains("# Another comment"));
    }

    #[test]
    fn test_parse_from_str_section_counts() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- ASSERTS ---
.x == 1
";
        let doc = parse_gctf_from_str(input, "test.gctf").unwrap();
        assert_eq!(doc.sections.len(), 3);
        assert!(doc.get_endpoint().is_some());
        let asserts = doc.get_assertions();
        assert_eq!(asserts.len(), 1);
    }

    #[test]
    fn test_extract_doc_source() {
        let source = "line0\nline1\nline2\nline3\nline4";
        let lines: Vec<&str> = source.lines().collect();
        let sections = vec![Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("line1".into()),
            inline_options: InlineOptions::default(),
            raw_content: "line1".into(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
        }];
        let result = extract_doc_source_from_lines(&sections, &lines);
        assert_eq!(result, "line1");
    }

    #[test]
    fn test_extract_doc_source_empty() {
        let result = extract_doc_source_from_lines(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_attribute_before_section_attaches_to_following_section() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

#[name(test)]
--- REQUEST ---
{}

--- RESPONSE ---
{}
";

        let (sections, _) = parse_sections_from_str(input).unwrap();
        assert_eq!(sections.len(), 3);

        let endpoint = &sections[0];
        let request = &sections[1];

        assert!(endpoint.attributes.is_empty());
        assert_eq!(request.attributes.len(), 1);
        assert_eq!(request.attributes[0].name, "name");
        assert_eq!(request.attributes[0].value, "test");
    }

    #[test]
    fn test_attribute_between_sections_not_attached_to_previous_section() {
        let input = "\
--- ENDPOINT ---
test.Service/Method
#[timeout(10)]
--- REQUEST ---
{}
";

        let (sections, _) = parse_sections_from_str(input).unwrap();
        assert_eq!(sections.len(), 2);
        assert!(sections[0].attributes.is_empty());
        assert_eq!(sections[1].attributes.len(), 1);
        assert_eq!(sections[1].attributes[0].name, "timeout");
    }

    #[test]
    #[cfg(not(miri))]
    fn bench_parse_small_doc() {
        let header = "--- ENDPOINT ---
";
        let body = "test.Service/Method

--- REQUEST ---
{\"k\":\"v\"}

--- RESPONSE ---
{\"r\":\"ok\"}
";
        let input = format!("{}{}", header, body);
        let start = std::time::Instant::now();
        let n = 5000;
        for _ in 0..n {
            let _ = parse_sections_from_str(&input);
        }
        let d = start.elapsed();
        eprintln!("bench: {} iterations in {:?} ({:?}/call)", n, d, d / n);
    }
}