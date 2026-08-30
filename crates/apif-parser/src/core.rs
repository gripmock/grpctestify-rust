#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::content_parser::{self, parse_attribute};
use anyhow::{Context, Result};
use apif_ast::gctf_tokenizer::{GctfTokenKind, tokenize_gctf};
use apif_ast::*;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

type CurrentSection = Option<(
    SectionType,
    usize,
    Vec<String>,
    InlineOptions,
    Vec<GctfAttribute>,
)>;

pub fn parse_gctf(file_path: &Path) -> Result<GctfDocument> {
    let (document, _) = parse_gctf_with_diagnostics(file_path)?;
    Ok(document)
}

pub fn parse_gctf_from_str(content: &str, file_path: &str) -> Result<GctfDocument> {
    let (all_sections, _) =
        parse_sections_from_str_for(crate::ast::Family::of(file_path), content)?;
    let source_lines: Vec<&str> = content.lines().collect();

    let documents = crate::document_splitter::split_sections_by_boundary_owned(all_sections);

    if documents.is_empty() {
        let mut document = GctfDocument::new(file_path.to_string());
        document.metadata.source = Some(content.to_string());
        return Ok(document);
    }

    let mut head: Option<GctfDocument> = None;

    for doc_sections in documents.into_iter().rev() {
        let mut document = GctfDocument::new(file_path.to_string());
        document.metadata.source =
            Some(extract_doc_source_from_lines(&doc_sections, &source_lines));
        document.metadata.placeholder_free =
            doc_sections.iter().all(|s| !s.raw_content.contains("{{"));
        document.sections = doc_sections;
        document.next_document = head.map(Box::new);
        head = Some(document);
    }

    head.ok_or_else(|| anyhow::anyhow!("No documents parsed"))
}

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

pub fn parse_gctf_with_diagnostics(file_path: &Path) -> Result<(GctfDocument, ParseDiagnostics)> {
    let total_start = Instant::now();

    let read_start = Instant::now();
    let source = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
    let read_ms = read_start.elapsed().as_secs_f64() * 1000.0;

    let parse_sections_start = Instant::now();
    let (sections, section_headers) = parse_sections_from_str_for(
        crate::ast::Family::of(&file_path.to_string_lossy()),
        &source,
    )?;
    let parse_sections_ms = parse_sections_start.elapsed().as_secs_f64() * 1000.0;

    let documents = crate::document_splitter::split_sections_by_boundary_owned(sections);

    let build_start = Instant::now();
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

#[cfg(test)]
fn parse_sections_from_str(source: &str) -> Result<(Vec<Section>, usize)> {
    parse_sections_from_str_for(crate::ast::Family::Gctf, source)
}

fn parse_sections_from_str_for(
    family: crate::ast::Family,
    source: &str,
) -> Result<(Vec<Section>, usize)> {
    let tokens = tokenize_gctf(source);
    let line_offsets = line_start_byte_offsets(source);
    let mut sections = Vec::new();
    let mut section_headers = 0;
    let mut current_section: CurrentSection = None;
    let mut pending_attributes: Vec<GctfAttribute> = Vec::new();

    for token in tokens {
        match token.kind {
            GctfTokenKind::SectionHeader { name, raw_options } => {
                if let Some((section_type, start_line, content, options, raw_attrs)) =
                    current_section.take()
                {
                    let end_line = start_line + content.len() + 1;
                    let mut section = content_parser::build_section_for(
                        family,
                        section_type,
                        start_line,
                        end_line,
                        &content,
                        options,
                        raw_attrs,
                    )?;
                    section.span = SectionSpan::from_line_range(
                        &line_offsets,
                        start_line,
                        end_line,
                        source.len(),
                    );
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
                    return Err(match SectionType::nearest_keyword(&name) {
                        Some(meant) => anyhow::anyhow!(
                            "Unknown section type: {} — did you mean '{}'?",
                            name,
                            meant
                        ),
                        None => anyhow::anyhow!("Unknown section type: {}", name),
                    });
                }
            }
            GctfTokenKind::AttributeBlock(attr_content) => match parse_attribute(&attr_content) {
                Some(attr) => pending_attributes.push(attr),
                None => {
                    return Err(anyhow::anyhow!(
                        "Malformed attribute at line {}: #[{}]",
                        token.line,
                        attr_content
                    ));
                }
            },
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
        let mut section = content_parser::build_section_for(
            family,
            section_type,
            start_line,
            end_line,
            &content,
            options,
            raw_attrs,
        )?;
        section.span =
            SectionSpan::from_line_range(&line_offsets, start_line, end_line, source.len());
        sections.push(section);
    }
    Ok((sections, section_headers))
}

pub fn serialize_gctf(doc: &GctfDocument) -> String {
    write_gctf(doc, false)
}

pub fn serialize_gctf_as_written(doc: &GctfDocument) -> String {
    write_gctf(doc, true)
}

fn write_gctf(doc: &GctfDocument, as_written: bool) -> String {
    let mut output = serialize_one(doc, as_written);
    let mut next = doc.next_document.as_deref();
    while let Some(d) = next {
        output.push('\n');
        output.push_str(&serialize_one(d, as_written));
        next = d.next_document.as_deref();
    }
    output
}

fn section_as_written(section: &Section) -> Option<&str> {
    let raw = section.raw_content.trim_end();
    if raw.trim().is_empty() {
        return None;
    }
    let reread = content_parser::parse_section_content(section.section_type, raw).ok()?;
    (reread == section.content).then_some(raw)
}

fn serialize_one(doc: &GctfDocument, as_written: bool) -> String {
    use std::fmt::Write;
    let mut output = String::new();

    let sections = sort_sections_for_fmt(&doc.sections);

    for section in &sections {
        for attr in &section.attributes {
            let _ = writeln!(output, "{}", attr.format_directive());
        }

        let _ = write!(output, "{}", section.format_header());
        output.push('\n');

        if as_written && let Some(raw) = section_as_written(section) {
            let _ = writeln!(output, "{}", raw);
            output.push('\n');
            continue;
        }

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
                for (k, v) in vars.iter() {
                    let _ = writeln!(output, "{} = {}", k, v);
                }
            }
            SectionContent::Meta(meta) => {
                if let Ok(yaml) = serde_yaml_ng::to_string(meta) {
                    let _ = writeln!(output, "{}", yaml.trim_end());
                }
            }
            SectionContent::Rows(rows) => {
                if let Ok(yaml) = serde_yaml_ng::to_string(rows) {
                    let _ = writeln!(output, "{}", yaml.trim_end());
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

    let mut preamble: Vec<&Section> = sections
        .iter()
        .filter(|s| s.section_type.preamble_rank().is_some())
        .collect();
    preamble.sort_by_key(|s| s.section_type.preamble_rank().unwrap());

    let mut result = Vec::with_capacity(sections.len());
    for s in &preamble {
        result.push((*s).clone());
    }
    for s in sections
        .iter()
        .filter(|s| s.section_type.preamble_rank().is_none())
    {
        result.push(s.clone());
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
        "concurrency_schedule",
        "concurrency_start",
        "concurrency_end",
        "concurrency_step",
        "concurrency_step_duration",
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

    #[test]
    fn serializing_keeps_the_options_a_section_was_read_with() {
        let source = "--- ENDPOINT ---\na.A/One\n\n--- REQUEST ---\n{\"id\": \"1\"}\n\n\
--- RESPONSE partial=true tolerance=0.01 redact=[\"token\"] with_asserts=true ---\n{\"ok\": true}\n\n\
--- ASSERTS ---\n.ok == true\n";
        let doc = parse_gctf_from_str(source, "t.gctf").unwrap();
        let again = parse_gctf_from_str(&serialize_gctf(&doc), "t.gctf").unwrap();

        let before = doc.first_section(SectionType::Response).unwrap();
        let after = again.first_section(SectionType::Response).unwrap();
        assert_eq!(before.inline_options, after.inline_options);
        assert!(after.inline_options.partial);
        assert_eq!(after.inline_options.tolerance, Some(0.01));
        assert_eq!(after.inline_options.redact, vec!["token".to_string()]);
        assert!(after.inline_options.with_asserts);
    }

    use super::*;

    #[test]
    fn parse_sections_basic() {
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
    fn malformed_attribute_is_a_hard_error_in_strict_path() {
        let input = "--- ENDPOINT ---\nsvc/Method\n#[]\n--- REQUEST ---\n{}\n";
        let err = parse_sections_from_str(input).unwrap_err();
        assert!(err.to_string().contains("Malformed attribute"), "{err}");
    }

    #[test]
    fn section_span_slices_back_to_the_sections_own_source_text() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
";
        let (sections, _) = parse_sections_from_str(input).unwrap();
        let request = sections
            .iter()
            .find(|s| s.section_type == SectionType::Request)
            .unwrap();
        let sliced = &input[request.span.start_byte..request.span.end_byte];
        assert_eq!(sliced, "--- REQUEST ---\n{}\n\n");
        assert_eq!(request.span.start_line, request.start_line);
        assert_eq!(request.span.end_line, request.end_line);
    }

    #[test]
    fn section_end_line_excludes_attribute_lines_of_next_section() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

#[timeout(5)]
--- RESPONSE ---
{}
";
        let (sections, _) = parse_sections_from_str(input).unwrap();
        let request = sections
            .iter()
            .find(|s| s.section_type == SectionType::Request)
            .unwrap();
        assert_eq!(
            request.end_line, 6,
            "REQUEST must not swallow the #[timeout] line ahead of RESPONSE"
        );
    }

    #[test]
    fn section_header_tokenizer() {
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
    fn parse_multi_document() {
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
    fn parse_empty_content() {
        let doc = parse_gctf_from_str("", "test.gctf").unwrap();
        assert!(doc.sections.is_empty());
    }

    #[test]
    fn parse_all_section_types() {
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
    fn parse_unknown_section_type() {
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
    fn parse_preserves_comments_in_content() {
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
    fn parse_from_str_section_counts() {
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
    fn extract_doc_source() {
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
            span: SectionSpan::default(),
        }];
        let result = extract_doc_source_from_lines(&sections, &lines);
        assert_eq!(result, "line1");
    }

    #[test]
    fn extract_doc_source_empty() {
        let result = extract_doc_source_from_lines(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn attribute_before_section_attaches_to_following_section() {
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
    fn attribute_between_sections_not_attached_to_previous_section() {
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
    fn a_preserved_section_is_written_as_its_author_wrote_it() {
        let src = "--- ENDPOINT ---\npkg.Svc/M\n\n--- OPTIONS ---\ntimeout: 7\nretry: 2\n\n--- ASSERTS ---\n# why this matters\n.ok == true\n";
        let doc = parse_gctf_from_str(src, "t.gctf").expect("parse");

        let as_written = serialize_gctf_as_written(&doc);
        assert!(as_written.contains("# why this matters"), "{as_written}");
        assert!(as_written.contains("timeout: 7\nretry: 2"), "{as_written}");

        let canonical = serialize_gctf(&doc);
        assert!(!canonical.contains("# why this matters"), "{canonical}");
        assert!(canonical.contains("retry: 2\ntimeout: 7"), "{canonical}");

        assert_eq!(
            parse_gctf_from_str(&as_written, "t.gctf")
                .expect("reparse")
                .get_options(),
            doc.get_options(),
            "the author's text still means what it meant"
        );
    }

    #[test]
    fn dataset_section_round_trips_through_serialize_gctf() {
        let input = "\
--- ENDPOINT ---
test.Service/Method

--- DATASET ---
- id: '1'
  name: Ada
- id: '2'
  name: Grace

--- REQUEST ---
{\"id\": \"{{dataset.id}}\"}

--- RESPONSE ---
{}
";
        let doc = parse_gctf_from_str(input, "test.gctf").unwrap();
        let section = doc.first_section(SectionType::Dataset).unwrap();
        let SectionContent::Rows(rows) = &section.content else {
            panic!("expected Rows content");
        };
        assert_eq!(rows.len(), 2);

        let serialized = serialize_gctf(&doc);
        assert!(serialized.contains("--- DATASET ---"));

        let reparsed = parse_gctf_from_str(&serialized, "test.gctf").unwrap();
        let reparsed_section = reparsed.first_section(SectionType::Dataset).unwrap();
        let SectionContent::Rows(reparsed_rows) = &reparsed_section.content else {
            panic!("expected Rows content after round-trip");
        };
        assert_eq!(reparsed_rows, rows);
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
