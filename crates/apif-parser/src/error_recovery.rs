// Error recovery parser for GCTF files
// Parses as much as possible and collects all errors

use crate::assertions::strip_assertion_comments;
use crate::ast::{DocumentMetadata, FileMeta, GctfDocument, Section, SectionContent, SectionType};
use crate::gctf_tokenizer;
use apif_diagnostics::{DiagnosticCode, DiagnosticCollection, Range};
use std::path::Path;

pub struct ErrorRecoveryResult {
    pub document: GctfDocument,
    pub diagnostics: DiagnosticCollection,
    pub recovered_sections: usize,
    pub failed_sections: usize,
}

/// Parse GCTF file with error recovery.
/// Supports multiple documents via document chain. An unreadable file (missing,
/// permission denied, non-UTF8) is not silently treated as empty — it surfaces
/// as an error diagnostic on the (otherwise empty) result, same as any other
/// recovered-from parse failure in this module.
pub fn parse_with_recovery(file_path: &Path) -> ErrorRecoveryResult {
    let file_path_str = file_path.to_string_lossy();
    match std::fs::read_to_string(file_path) {
        Ok(content) => parse_content_with_recovery(&content, file_path_str.as_ref()),
        Err(e) => {
            let mut diagnostics = DiagnosticCollection::new();
            diagnostics.error(
                DiagnosticCode::InvalidSectionContent,
                format!("Failed to read file {}: {}", file_path.display(), e),
                Range::at_line(0),
            );
            ErrorRecoveryResult {
                document: GctfDocument::new(file_path_str.to_string()),
                diagnostics,
                recovered_sections: 0,
                failed_sections: 0,
            }
        }
    }
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
        },
        next_document: None,
    }
}

/// A section being accumulated while walking the token stream.
struct PendingSection {
    section_type: SectionType,
    inline_options: crate::ast::InlineOptions,
    /// Source line of this section's `--- X ---` header.
    header_line: usize,
    /// Raw content lines (Content/Comment/Blank tokens), excluding `#[attr]`
    /// lines (those are diverted to the next section's attributes, same as the
    /// strict path).
    content_lines: Vec<String>,
    /// Source line of the last content line added — the inclusive `end_line`,
    /// matching this module's long-standing convention (the strict path uses
    /// an exclusive bound instead; the two still differ by design, see the
    /// span note in `finalize_section`).
    last_line: usize,
    attributes: Vec<crate::ast::GctfAttribute>,
}

/// Parse the whole content as one flat document; the caller
/// (`parse_content_with_recovery`) is what splits it on ENDPOINT boundaries.
///
/// Consumes the shared `gctf_tokenizer` token stream — the same one the strict
/// `core.rs` path uses — instead of hand-rolling raw line scanning, so this
/// module no longer reads raw `.gctf` text directly (the sole remaining raw
/// scan, detecting a miscased `--- endpoint ---` header, lives in the
/// tokenizer via `scan_miscased_section_header_name`). Recovery behavior is
/// preserved: nothing hard-fails, every malformed input becomes a diagnostic.
fn parse_single_with_recovery(content: &str, file_path: &str) -> ErrorRecoveryResult {
    let mut diagnostics = DiagnosticCollection::new();
    let mut sections = Vec::new();
    let mut recovered_sections = 0;
    let failed_sections = 0;

    let tokens = gctf_tokenizer::tokenize_gctf(content);
    let line_offsets = crate::ast::line_start_byte_offsets(content);

    // Uniform deprecation detection (HEADERS alias, kebab OPTIONS keys, kebab
    // attributes) from the one shared token-level detector — the same source
    // the strict `check` path uses, so every command reports identically.
    for diag in crate::deprecations::detect_deprecations(&tokens) {
        diagnostics.push(diag);
    }

    let mut current: Option<PendingSection> = None;
    let mut pending_attributes: Vec<crate::ast::GctfAttribute> = Vec::new();

    for token in &tokens {
        match &token.kind {
            gctf_tokenizer::GctfTokenKind::SectionHeader { name, raw_options } => {
                if let Some(pending) = current.take() {
                    finalize_section(
                        pending,
                        content,
                        &line_offsets,
                        &mut sections,
                        &mut diagnostics,
                    );
                    recovered_sections += 1;
                }

                match SectionType::from_keyword(name) {
                    Some(section_type) => {
                        // (HEADERS-alias deprecation is reported once, up front,
                        // by the shared `detect_deprecations` pass above.)
                        let inline_options = resolve_inline_options(
                            section_type,
                            raw_options,
                            token.line,
                            &mut diagnostics,
                        );
                        current = Some(PendingSection {
                            section_type,
                            inline_options,
                            header_line: token.line,
                            content_lines: Vec::new(),
                            last_line: token.line,
                            attributes: std::mem::take(&mut pending_attributes),
                        });
                    }
                    None => {
                        // Uppercase-but-unknown section (e.g. `--- FOOBAR ---`).
                        // Pending attributes are left intact to attach to the
                        // next real section, matching the old behavior.
                        warn_unknown_section(name, token.line, &mut diagnostics);
                    }
                }
            }
            gctf_tokenizer::GctfTokenKind::AttributeBlock(attr_content) => {
                match crate::content_parser::parse_attribute(attr_content) {
                    Some(attr) => pending_attributes.push(attr),
                    None => diagnostics.warning(
                        DiagnosticCode::InvalidSyntax,
                        format!("Malformed attribute: #[{}]", attr_content),
                        Range::at_line(token.line),
                    ),
                }
            }
            gctf_tokenizer::GctfTokenKind::Comment(text)
            | gctf_tokenizer::GctfTokenKind::Content(text) => {
                if let Some(pending) = current.as_mut() {
                    pending.content_lines.push(text.clone());
                    pending.last_line = token.line;
                } else if matches!(token.kind, gctf_tokenizer::GctfTokenKind::Content(_)) {
                    // Floating (not inside any section) content line that is
                    // actually a miscased/invalid section-header shape — give
                    // the same "did you mean 'ENDPOINT'?" diagnostic the strict
                    // uppercase-only tokenizer can't. A comment or genuine
                    // content line here is silently dropped, as before.
                    if let Some(raw_name) = gctf_tokenizer::scan_miscased_section_header_name(text)
                    {
                        warn_unknown_section(&raw_name, token.line, &mut diagnostics);
                    }
                }
            }
            gctf_tokenizer::GctfTokenKind::Blank => {
                if let Some(pending) = current.as_mut() {
                    pending.content_lines.push(String::new());
                    pending.last_line = token.line;
                }
            }
        }
    }

    if let Some(pending) = current.take() {
        finalize_section(
            pending,
            content,
            &line_offsets,
            &mut sections,
            &mut diagnostics,
        );
        recovered_sections += 1;
    }

    let document = GctfDocument {
        file_path: file_path.to_string(),
        sections,
        metadata: DocumentMetadata {
            source: Some(content.to_string()),
            mtime: None,
            parsed_at: 0,
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

/// Turn an accumulated `PendingSection` into a `Section`, parsing its content
/// with recovery and computing its span.
fn finalize_section(
    pending: PendingSection,
    content: &str,
    line_offsets: &[usize],
    sections: &mut Vec<Section>,
    diagnostics: &mut DiagnosticCollection,
) {
    let content_start = pending.header_line + 1;
    let content_result = parse_section_content(
        &pending.content_lines,
        content_start,
        pending.section_type,
        diagnostics,
    );

    // `end_line` is the inclusive last-content-line index (this module's
    // convention); the span wants a half-open bound, hence `+ 1`.
    let end_line = pending.last_line;
    let span = crate::ast::SectionSpan::from_line_range(
        line_offsets,
        pending.header_line,
        end_line + 1,
        content.len(),
    );

    sections.push(Section {
        section_type: pending.section_type,
        content: content_result,
        inline_options: pending.inline_options,
        raw_content: pending.content_lines.join("\n"),
        start_line: pending.header_line,
        end_line,
        attributes: pending.attributes,
        span,
    });
}

/// Resolve a header's inline options, recovering (diagnostic + default) on any
/// problem instead of failing — mirrors the strict path's option parsing but
/// never propagates an error.
fn resolve_inline_options(
    section_type: SectionType,
    raw_options: &str,
    line_num: usize,
    diagnostics: &mut DiagnosticCollection,
) -> crate::ast::InlineOptions {
    let has_opts = !raw_options.is_empty();
    if has_opts && section_type.supports_inline_options() {
        match crate::content_parser::parse_inline_options(raw_options) {
            Ok(opts) => opts,
            Err(_) => {
                parse_inline_options_diagnostic(raw_options, line_num, diagnostics);
                Default::default()
            }
        }
    } else {
        if has_opts {
            parse_inline_options_diagnostic(raw_options, line_num, diagnostics);
        }
        Default::default()
    }
}

/// Warn about a section name that isn't a known type. A recognizable-but-
/// miscased name (`endpoint` → `ENDPOINT`) gets an actionable case hint; a
/// genuinely unknown name gets a plain "unknown section type".
fn warn_unknown_section(name: &str, line_num: usize, diagnostics: &mut DiagnosticCollection) {
    let upper = name.to_uppercase();
    let message = if name != upper && SectionType::from_keyword(&upper).is_some() {
        format!(
            "Unknown section type: '{name}' — section names are case-sensitive, did you mean '{upper}'?"
        )
    } else {
        format!("Unknown section type: {name}")
    };
    diagnostics.warning(
        DiagnosticCode::UnknownSectionType,
        message,
        Range::at_line(line_num),
    );
}

/// Parse section content based on type
fn parse_section_content(
    content: &[String],
    start_line: usize,
    section_type: SectionType,
    diagnostics: &mut DiagnosticCollection,
) -> SectionContent {
    let content_str = content.join("\n");

    // Same early exit as the strict path, so an empty section has one
    // representation rather than two (`Empty` there, a typed-but-empty value
    // here).
    if content_str.trim().is_empty() {
        return SectionContent::Empty;
    }

    match section_type {
        // Same rule as the strict path: a `//`/`#` line is a comment, never
        // part of the dialed address. This is the path `run` takes.
        SectionType::Address | SectionType::Endpoint => {
            let stripped = crate::gctf_tokenizer::strip_gctf_comment_lines(&content_str);
            let stripped = stripped.trim();
            if stripped.is_empty() {
                SectionContent::Empty
            } else {
                SectionContent::Single(stripped.to_string())
            }
        }
        SectionType::Request | SectionType::Response | SectionType::Error => {
            if content_str.trim().is_empty() {
                SectionContent::Empty
            } else {
                // Try to parse as JSON5 (with comments), but don't fail - just add diagnostic
                match crate::json_mod::from_str(&content_str) {
                    Ok(value) => SectionContent::Json(value),
                    // Multiple payloads in one section is the streaming form;
                    // ERROR stays single-value, matching the strict path.
                    Err(_)
                        if section_type != SectionType::Error
                            && let Some(values) =
                                crate::json_stream_parser::parse_response_json_values(
                                    &content_str,
                                ) =>
                    {
                        SectionContent::JsonLines(values)
                    }
                    Err(e) => {
                        // `content_str` is unmodified `content.join("\n")`, so
                        // its line 0 is exactly file line `start_line` — the
                        // parser's own relative line, if it reported one,
                        // can be turned into an absolute file line by simple
                        // addition rather than pointing at the section start
                        // regardless of where the actual error is.
                        let line = e
                            .downcast_ref::<crate::json_mod::JsonParseError>()
                            .and_then(|err| err.line)
                            .map(|relative_line| start_line + relative_line)
                            .unwrap_or(start_line);
                        diagnostics.error(
                            DiagnosticCode::JsonParseError,
                            format!("Failed to parse JSON: {}", e),
                            Range::at_line(line),
                        );
                        // Never smuggle the unparsed raw text as a JSON string
                        // value — a consumer that doesn't check diagnostics
                        // would otherwise treat garbage text as a real payload
                        // (e.g. send it literally as a gRPC request body).
                        SectionContent::Empty
                    }
                }
            }
        }
        SectionType::Extract => {
            let mut extractions = crate::ast::OrderedStringMap::new();
            for (i, line) in content.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }

                // Recognition (not just splitting) goes through the shared
                // tokenizer, same as the strict path's `content_parser.rs` —
                // this file used to hand-roll its own `.find('=')` here.
                if let Some((name, query)) = gctf_tokenizer::tokenize_extract_line(line) {
                    if extractions.contains_key(&name) {
                        diagnostics.warning(
                            DiagnosticCode::DuplicateKey,
                            format!(
                                "Duplicate EXTRACT variable '{name}' — only the last assignment is kept"
                            ),
                            Range::at_line(start_line + i),
                        );
                    }
                    // Store the jq form, as the strict path does.
                    let value = crate::ternary_ast::ExtractVar::parse_raw(&name, &query)
                        .map(|var| var.value.to_jq())
                        .unwrap_or(query);
                    extractions.insert(name, value);
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
            let assertions: Vec<String> = content
                .iter()
                .filter_map(|line| strip_assertion_comments(line))
                .collect();
            SectionContent::Assertions(assertions)
        }
        SectionType::RequestHeaders
        | SectionType::Tls
        | SectionType::Proto
        | SectionType::Options => {
            let mut key_values = crate::ast::OrderedStringMap::new();
            for (i, line) in content.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }

                // Recognition (not just splitting) goes through the shared
                // tokenizer, same as the strict path's `content_parser.rs` —
                // this file used to hand-roll its own `.find(':')` here.
                if let Some((key, value)) = gctf_tokenizer::tokenize_kv_line(line) {
                    if key_values.contains_key(&key) {
                        diagnostics.warning(
                            DiagnosticCode::DuplicateKey,
                            format!("Duplicate key '{key}' — only the last value is kept"),
                            Range::at_line(start_line + i),
                        );
                    }
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
        // `sources:` carries a nested YAML list on continuation lines, which
        // the flat loop above drops. The strict parser handles it but skips
        // untokenizable lines silently, so their warning is re-emitted below.
        SectionType::Bench => {
            let kv = crate::content_parser::parse_bench_section(&content_str).unwrap_or_else(|e| {
                diagnostics.warning(
                    DiagnosticCode::InvalidSectionContent,
                    format!("Failed to parse BENCH section: {e}"),
                    Range::at_line(start_line),
                );
                crate::ast::OrderedStringMap::new()
            });
            for (i, line) in content.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.starts_with('#')
                    || trimmed.starts_with("//")
                    // An indented line continues the previous key's value.
                    || line.starts_with(' ')
                    || line.starts_with('\t')
                {
                    continue;
                }
                if gctf_tokenizer::tokenize_kv_line(line).is_none() {
                    diagnostics.warning(
                        DiagnosticCode::InvalidSyntax,
                        "Invalid key-value syntax, expected: key: value",
                        Range::at_line(start_line + i),
                    );
                }
            }
            SectionContent::KeyValues(kv)
        }
        SectionType::Meta => {
            // Strip GCTF comment lines before parsing YAML — shared with the
            // strict path so both accept the same comment styles.
            let cleaned = gctf_tokenizer::strip_gctf_comment_lines(&content.join("\n"));
            let meta = match serde_yaml_ng::from_str::<FileMeta>(&cleaned) {
                Ok(meta) => meta,
                Err(e) => {
                    diagnostics.error(
                        DiagnosticCode::InvalidSectionContent,
                        format!("Invalid META: {e}"),
                        Range::at_line(start_line),
                    );
                    FileMeta::default()
                }
            };
            SectionContent::Meta(meta)
        }
        SectionType::Dataset => {
            // Same comment-stripping as META; recovers rather than failing,
            // but still reports — a silent default here meant zero rows.
            let cleaned = gctf_tokenizer::strip_gctf_comment_lines(&content.join("\n"));
            let rows: Vec<serde_json::Value> = match serde_yaml_ng::from_str(&cleaned) {
                Ok(rows) => rows,
                Err(e) => {
                    diagnostics.error(
                        DiagnosticCode::InvalidSectionContent,
                        format!("DATASET must be a YAML list of row objects: {e}"),
                        Range::at_line(start_line),
                    );
                    Vec::new()
                }
            };
            // Strict rejects a non-object row; here it is dropped and reported.
            let mut kept = Vec::with_capacity(rows.len());
            for (i, row) in rows.into_iter().enumerate() {
                if row.is_object() {
                    kept.push(row);
                } else {
                    diagnostics.error(
                        DiagnosticCode::InvalidSectionContent,
                        format!("DATASET row {i} must be an object, got: {row}"),
                        Range::at_line(start_line),
                    );
                }
            }
            SectionContent::Rows(kept)
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
                _ if crate::content_parser::is_extra_inline_option_key(key) => {}
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

    // The lenient path is the one `run` takes, so this is where a comment
    // leaking into the address actually changed the dial target.
    #[test]
    fn a_comment_line_is_not_part_of_a_single_value_section() {
        let mut diagnostics = DiagnosticCollection::new();
        let content = vec!["// staging only".to_string(), "localhost:4770".to_string()];
        assert_eq!(
            parse_section_content(&content, 1, SectionType::Address, &mut diagnostics),
            SectionContent::Single("localhost:4770".to_string())
        );
        assert_eq!(
            parse_section_content(
                &["// note".to_string()],
                1,
                SectionType::Address,
                &mut diagnostics
            ),
            SectionContent::Empty
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_with_recovery_unreadable_file_yields_error_diagnostic() {
        // §3.5: a missing/unreadable file must not silently become an empty
        // document with zero diagnostics — it carries an IO-error diagnostic.
        let result = parse_with_recovery(Path::new("/no/such/path/definitely-missing-4f3a.gctf"));
        assert!(result.document.sections.is_empty());
        assert!(result.diagnostics.has_errors());
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Failed to read file")),
            "{:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn parse_with_recovery_invalid_json_yields_empty_content_not_raw_text() {
        // §3.6: an invalid JSON body becomes `SectionContent::Empty` (plus a
        // diagnostic), never the raw unparsed text smuggled as a JSON string
        // — a consumer ignoring diagnostics must not receive garbage as a
        // real payload.
        let content = "--- ENDPOINT ---\nsvc/Method\n\n--- REQUEST ---\n{not valid json\n";
        let result = parse_content_with_recovery(content, "test.gctf");
        let request = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Request)
            .expect("REQUEST section recovered");
        assert!(
            matches!(request.content, SectionContent::Empty),
            "invalid JSON must yield Empty, got {:?}",
            request.content
        );
        assert!(result.diagnostics.has_errors());
    }

    #[test]
    fn parse_with_recovery_malformed_attribute_warns() {
        // §3.4 lenient counterpart: a malformed `#[]` attribute emits a
        // warning instead of being silently dropped.
        let content = "--- ENDPOINT ---\nsvc/Method\n#[]\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Malformed attribute")),
            "{:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn parse_with_recovery_malformed_meta_yields_diagnostic() {
        // §3.1 lenient counterpart: malformed META YAML emits an error
        // diagnostic (and recovers with a default FileMeta), never silent.
        let content = "--- META ---\nname: [unterminated\n\n--- ENDPOINT ---\nsvc/Method\n\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Invalid META")),
            "{:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn lenient_section_span_slices_back_to_the_sections_own_source_text() {
        let content = "--- ENDPOINT ---\nservice/Method\n\n--- REQUEST ---\n{\"key\": \"value\"}\n\n--- RESPONSE ---\n{\"result\": \"ok\"}\n";
        let result = parse_single_with_recovery(content, "test.gctf");
        let request = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == crate::ast::SectionType::Request)
            .expect("REQUEST section recovered");

        let sliced = &content[request.span.start_byte..request.span.end_byte];
        assert_eq!(sliced, "--- REQUEST ---\n{\"key\": \"value\"}\n\n");
        assert_eq!(request.span.start_line, request.start_line);
        // Unlike the strict path, this module's `Section.end_line` is the
        // *inclusive* last content line — `span.end_line` is the correct
        // half-open bound (`end_line + 1`), so they're off by one here by
        // design, not a bug.
        assert_eq!(request.span.end_line, request.end_line + 1);
    }

    #[test]
    fn token_based_recovery_attaches_attributes_and_preserves_content() {
        // After the token-stream rewrite, `#[attr]` blocks between sections
        // must still attach to the *following* section (not the preceding one)
        // and must not leak into any section's content — the same behavior the
        // old hand-rolled `extract_section_content` produced.
        let content = r#"--- ENDPOINT ---
svc/Method

#[timeout(5)]
#[retry(2)]
--- REQUEST ---
{"a": 1}

--- RESPONSE ---
{"b": 2}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(result.recovered_sections, 3);

        let endpoint = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Endpoint)
            .expect("ENDPOINT section");
        // The attributes belong to REQUEST, not ENDPOINT.
        assert!(endpoint.attributes.is_empty());

        let request = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Request)
            .expect("REQUEST section");
        assert_eq!(request.attributes.len(), 2);
        assert_eq!(request.attributes[0].name, "timeout");
        assert_eq!(request.attributes[1].name, "retry");
        // Attribute lines never leak into the section's raw content.
        assert!(!request.raw_content.contains("#["));
        assert!(request.raw_content.contains("{\"a\": 1}"));
    }

    #[test]
    fn parse_with_recovery_valid_file() {
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
    fn parse_with_recovery_invalid_json() {
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
    fn parse_with_recovery_invalid_json_points_at_the_actual_line() {
        // Line 5 (0-based) is where the malformed token actually is, not
        // line 4 (the REQUEST section's own start) — a JSON parse error used
        // to always report the section start regardless of where inside a
        // multi-line body the real problem was.
        let content = r#"--- ENDPOINT ---
service/Method

--- REQUEST ---
{
  "key": invalid_token
}

--- RESPONSE ---
{"result": "ok"}
"#;

        let result = parse_content_with_recovery(content, "test.gctf");

        assert!(result.diagnostics.has_errors());
        let json_error = result
            .diagnostics
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::JsonParseError)
            .expect("a JSON parse error diagnostic");
        assert_eq!(json_error.range.start.line, 5);
    }

    #[test]
    fn parse_with_recovery_multiple_errors() {
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
    fn parse_with_recovery_unknown_section() {
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
    fn parse_with_recovery_lowercase_section_name_rejected_like_strict_path() {
        // Decided: section names are case-sensitive everywhere, not just in
        // the strict path — `check`/`fmt` never recognized `--- endpoint ---`
        // (`gctf_tokenizer::is_section_name_char` is uppercase-only), and
        // this lenient path used to silently case-fold and accept it, so the
        // exact same file behaved differently depending on which command
        // touched it. Now both agree: lowercase is rejected, not recovered.
        let content = r#"--- endpoint ---
svc/Method

--- request ---
{}

--- response ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(
            result.recovered_sections, 0,
            "lowercase section names must not recover as any SectionType"
        );
        assert!(
            !result
                .document
                .sections
                .iter()
                .any(|s| s.section_type == SectionType::Endpoint),
        );
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .filter(|d| d.message.contains("did you mean 'ENDPOINT'"))
                .count()
                == 1,
            "expected a case-mismatch hint for the recognizable-but-miscased name: {:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn parse_with_recovery_unknown_section_name_not_double_warned() {
        // A genuinely unknown section (not just miscased) must get exactly
        // one diagnostic, not both "should be uppercase" and "unknown
        // section type" for the same line.
        let content = "--- garbage ---\nfoo\n";
        let result = parse_content_with_recovery(content, "test.gctf");
        assert_eq!(
            result.diagnostics.diagnostics.len(),
            1,
            "{:?}",
            result.diagnostics.diagnostics
        );
        assert!(
            result.diagnostics.diagnostics[0]
                .message
                .contains("Unknown section type")
        );
    }

    #[test]
    fn parse_with_recovery_invalid_extract() {
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
    fn parse_with_recovery_asserts_double_slash_comments() {
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
    fn parse_with_recovery_asserts_inline_comments() {
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
    fn parse_with_recovery_headers_deprecated() {
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
    fn parse_with_recovery_tls_section_key_values() {
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
    fn parse_with_recovery_options_section() {
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
    fn parse_with_recovery_duplicate_options_key_warns_and_last_wins() {
        let content = r#"--- OPTIONS ---
timeout: 30
timeout: 60

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Duplicate key 'timeout'")),
            "{:?}",
            result.diagnostics.diagnostics
        );
        let opts = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Options)
            .unwrap();
        if let SectionContent::KeyValues(kvs) = &opts.content {
            assert_eq!(kvs.get("timeout"), Some(&"60".to_string()));
        } else {
            panic!("expected key-values content");
        }
    }

    #[test]
    fn parse_with_recovery_duplicate_extract_variable_warns() {
        let content = r#"--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- EXTRACT ---
total = .a
total = .b
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(result.diagnostics.has_warnings());
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Duplicate EXTRACT variable 'total'")),
            "{:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn parse_with_recovery_bench_continuation_line_not_flagged_as_duplicate() {
        // `sources:`'s nested YAML list uses indented continuation lines,
        // not repeated top-level keys — must not warn.
        let content = r#"--- BENCH ---
mode: fixed
sources:
  - name: a
    file: a.csv
  - name: b
    file: b.csv

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(
            !result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Duplicate key")),
            "{:?}",
            result.diagnostics.diagnostics
        );
    }

    #[test]
    fn parse_with_recovery_proto_section() {
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
    fn parse_with_recovery_empty_response() {
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
    fn parse_with_recovery_non_section_header_lines() {
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
    fn parse_with_recovery_comment_lines() {
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
    fn parse_with_recovery_inline_options_invalid_boolean() {
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
    fn parse_with_recovery_inline_options_invalid_numeric() {
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
    fn parse_with_recovery_inline_options_unknown() {
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
    fn parse_with_recovery_inline_options_valid() {
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
    fn parse_with_recovery_request_headers_section() {
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
    fn parse_with_recovery_invalid_key_value_syntax() {
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

    #[test]
    fn parse_with_recovery_kv_and_extract_recognize_slash_comments() {
        // Regression: this file used to hand-roll `.find(':')`/`.find('=')`
        // for KV/EXTRACT lines, only skipping `#`-comments — a `//` comment
        // (the tokenizer-recognized form `content_parser.rs`'s strict path
        // already handles via `tokenize_kv_line`/`tokenize_extract_line`)
        // fell through to "Invalid syntax" instead of being silently
        // skipped. Now shares those same tokenizer functions.
        let content = r#"--- OPTIONS ---
// a real comment, not a key
timeout: 30

--- ENDPOINT ---
svc/Method

--- REQUEST ---
{}

--- RESPONSE ---
{}

--- EXTRACT ---
// another comment
status = .status
"#;
        let result = parse_content_with_recovery(content, "test.gctf");
        assert!(
            !result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Invalid")),
            "{:?}",
            result.diagnostics.diagnostics
        );
        let options = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Options)
            .unwrap();
        if let SectionContent::KeyValues(kv) = &options.content {
            assert_eq!(kv.len(), 1);
            assert_eq!(kv.get("timeout"), Some(&"30".to_string()));
        } else {
            panic!("expected KeyValues");
        }
    }

    #[test]
    fn bench_sources_nested_list_survives_recovery() {
        let content = "--- ENDPOINT ---\npkg.Svc/Method\n\n--- BENCH ---\nmode: fixed\nsources:\n  - name: users\n    file: data/users.csv\n    indexed_by: id\n\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "t.gctf");
        let bench = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Bench)
            .expect("BENCH section");
        let SectionContent::KeyValues(kv) = &bench.content else {
            panic!("BENCH must be key-values");
        };

        assert_eq!(kv.get("mode").map(String::as_str), Some("fixed"));
        let sources = kv.get("sources").expect("sources key must be present");
        assert!(
            sources.contains("name: users") && sources.contains("data/users.csv"),
            "the nested list must stay attached to `sources`, got: {sources:?}"
        );
        // The list items must not leak out as top-level keys.
        assert!(
            !kv.contains_key("- name"),
            "nested list leaked a key: {kv:?}"
        );
        assert!(!kv.contains_key("file"), "nested list leaked a key: {kv:?}");
        assert!(
            !kv.contains_key("indexed_by"),
            "nested list leaked a key: {kv:?}"
        );
    }

    #[test]
    fn streaming_json_lines_survive_recovery() {
        let content = "--- ENDPOINT ---\nchat.ChatService/SendMessages\n\n--- REQUEST ---\n{\n  \"text\": \"one\"\n}\n{\n  \"text\": \"two\"\n}\n{\n  \"text\": \"three\"\n}\n\n--- RESPONSE ---\n{\n  \"count\": 3\n}\n";
        let result = parse_content_with_recovery(content, "t.gctf");

        let request = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Request)
            .expect("REQUEST section");
        match &request.content {
            SectionContent::JsonLines(values) => {
                assert_eq!(values.len(), 3, "all three messages must survive");
                assert_eq!(values[0]["text"], "one");
                assert_eq!(values[2]["text"], "three");
            }
            other => panic!("streaming REQUEST must parse as JsonLines, got {other:?}"),
        }

        let response = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Response)
            .expect("RESPONSE section");
        assert!(
            matches!(&response.content, SectionContent::Json(_)),
            "a single-value RESPONSE must still be plain Json"
        );

        assert!(
            !result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Failed to parse JSON")),
            "a valid streaming section must not report a JSON parse error"
        );
    }

    #[test]
    fn malformed_dataset_is_reported_not_swallowed() {
        let content = "--- ENDPOINT ---\nsvc.Service/Method\n\n--- DATASET ---\n- id: 1\n - broken indent\n\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "t.gctf");

        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("DATASET")),
            "an unparseable DATASET must produce a diagnostic, got: {:?}",
            result
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn non_object_dataset_row_is_reported_and_dropped() {
        let content = "--- ENDPOINT ---\nsvc.Service/Method\n\n--- DATASET ---\n- id: 1\n- 42\n\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "t.gctf");

        let dataset = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Dataset)
            .expect("DATASET section");
        let SectionContent::Rows(rows) = &dataset.content else {
            panic!("DATASET must be Rows");
        };
        assert_eq!(rows.len(), 1, "the scalar row must be dropped");
        assert_eq!(rows[0]["id"], 1);
        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("must be an object")),
            "dropping a row must be reported"
        );
    }

    #[test]
    fn probe_extract_ternary_divergence() {
        let content = "--- ENDPOINT ---\nsvc.Service/Method\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\nlabel = .code == 200 ? \"OK\" : \"Error\"\n";
        let rec = parse_content_with_recovery(content, "t.gctf");
        let sec = rec
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Extract)
            .unwrap();
        if let SectionContent::Extract(kv) = &sec.content {
            println!("RECOVERY label => {:?}", kv.get("label"));
        }
        let strict = crate::parse_gctf_from_str(content, "t.gctf").unwrap();
        let sec = strict
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Extract)
            .unwrap();
        if let SectionContent::Extract(kv) = &sec.content {
            println!("STRICT   label => {:?}", kv.get("label"));
        }
    }

    #[test]
    fn malformed_bench_line_is_still_reported() {
        let content = "--- ENDPOINT ---\nsvc.S/M\n\n--- BENCH ---\nmode fixed\nduration: 30s\nsources:\n  - name: u\n    file: u.csv\n\n--- REQUEST ---\n{}\n";
        let result = parse_content_with_recovery(content, "t.gctf");

        assert!(
            result
                .diagnostics
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Invalid key-value syntax")),
            "a BENCH line without a colon must be reported"
        );

        let bench = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Bench)
            .expect("BENCH section");
        let SectionContent::KeyValues(kv) = &bench.content else {
            panic!("BENCH must be key-values");
        };
        assert_eq!(kv.get("duration").map(String::as_str), Some("30s"));
        let sources = kv.get("sources").expect("sources survives");
        assert!(sources.contains("name: u"), "got {sources:?}");
    }

    #[test]
    fn extract_ternary_is_converted_to_jq_like_the_strict_path() {
        let content = "--- ENDPOINT ---\nsvc.S/M\n\n--- RESPONSE ---\n{}\n\n--- EXTRACT ---\nlabel = .code == 200 ? \"OK\" : \"Error\"\nplain = .id\n";
        let result = parse_content_with_recovery(content, "t.gctf");
        let section = result
            .document
            .sections
            .iter()
            .find(|s| s.section_type == SectionType::Extract)
            .expect("EXTRACT section");
        let SectionContent::Extract(kv) = &section.content else {
            panic!("EXTRACT must be Extract");
        };

        let label = kv.get("label").expect("label");
        assert!(
            label.contains("if") && label.contains("then") && label.contains("else"),
            "ternary must be converted to the jq form, got {label:?}"
        );
        assert_eq!(kv.get("plain").map(String::as_str), Some(".id"));
    }
}
