// AST (Abstract Syntax Tree) for .gctf files
// Represents the parsed structure of a .gctf test file

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Insertion-ordered string map backing KV/EXTRACT section content. Ordered
/// (not `HashMap`) so serialized output and every consumer iteration follow the
/// author's source order deterministically; keeps map ergonomics (`get`/dedup)
/// since duplicate keys are already rejected at parse time.
pub type OrderedStringMap = IndexMap<String, String>;

/// Complete .gctf document
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GctfDocument {
    /// File path (absolute or relative)
    pub file_path: String,

    /// All sections in the document (preserving order)
    pub sections: Vec<Section>,

    /// Document metadata
    pub metadata: DocumentMetadata,

    /// Next document in chain (for multi-document files)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_document: Option<Box<GctfDocument>>,
}

pub struct DocumentChainIter<'a> {
    current: Option<&'a GctfDocument>,
}

impl<'a> Iterator for DocumentChainIter<'a> {
    type Item = &'a GctfDocument;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        self.current = current.next_document.as_deref();
        Some(current)
    }
}

/// Document metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Original file content (for error reporting)
    pub source: Option<String>,

    /// File modification time (for caching)
    pub mtime: Option<i64>,

    /// Parsed at timestamp
    pub parsed_at: i64,
}

impl Default for DocumentMetadata {
    fn default() -> Self {
        Self {
            source: None,
            mtime: None,
            parsed_at: apif_cfg_runtime::now_timestamp(),
        }
    }
}

/// File-level metadata (META section)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FileMeta {
    /// Test name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Test summary (one-liner)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Test tags
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Test owner (team/person)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Related links (docs, jira, etc)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl FileMeta {
    /// Check if meta has any content
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.summary.is_none()
            && self.tags.is_empty()
            && self.owner.is_none()
            && self.links.is_empty()
    }
}

/// GCTF attribute (#[name(value)] syntax)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GctfAttribute {
    pub name: String,
    pub value: String,
}

impl GctfAttribute {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    pub fn flag(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: "true".into(),
        }
    }

    pub fn parse_u64(&self) -> Option<u64> {
        self.value.trim().parse::<u64>().ok()
    }

    pub fn parse_u32(&self) -> Option<u32> {
        self.value.trim().parse::<u32>().ok()
    }

    pub fn parse_f64(&self) -> Option<f64> {
        self.value.trim().parse::<f64>().ok()
    }

    pub fn parse_bool(&self) -> Option<bool> {
        match self.value.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" | "" => Some(false),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn format_directive(&self) -> String {
        let name = canonical_key_spelling(&self.name);
        if self.value == "true" {
            format!("#[{}]", name)
        } else {
            format!("#[{}({})]", name, self.value)
        }
    }
}

/// Deprecated kebab-case OPTIONS-key / `#[...]` attribute spellings that mean
/// the same thing as their canonical snake_case counterpart — kept working
/// indefinitely (never rejected), just normalized on the way back out and
/// flagged when seen. Single source of truth so a future rename only needs
/// one new entry here, instead of touching the validator's match arms and
/// every place that re-emits canonical output separately.
pub const DEPRECATED_KEBAB_CASE_KEYS: &[(&str, &str)] =
    &[("retry-delay", "retry_delay"), ("no-retry", "no_retry")];

/// Canonical spelling for `key`, or `key` itself if it's not a known
/// deprecated form.
pub fn canonical_key_spelling(key: &str) -> &str {
    DEPRECATED_KEBAB_CASE_KEYS
        .iter()
        .find(|(deprecated, _)| *deprecated == key)
        .map_or(key, |(_, canonical)| canonical)
}

impl Section {
    pub fn get_attribute(&self, name: &str) -> Option<&GctfAttribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    pub fn get_timeout(&self) -> Option<u64> {
        self.get_attribute("timeout")
            .and_then(|a| a.parse_u64())
            .filter(|&v| v > 0)
    }

    pub fn get_retry(&self) -> Option<u32> {
        self.get_attribute("retry").and_then(|a| a.parse_u32())
    }

    pub fn get_skip(&self) -> bool {
        self.get_attribute("skip")
            .and_then(|a| a.parse_bool())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.get_attribute("tag")
            .is_some_and(|a| a.value.split(',').any(|t| t.trim() == tag))
    }

    pub fn get_compression(&self) -> Option<String> {
        self.get_attribute("compression")
            .map(|a| a.value.trim().to_lowercase())
            .filter(|v| v == "gzip" || v == "none")
    }

    pub fn get_repeat(&self) -> Option<u32> {
        self.get_attribute("repeat")
            .and_then(|a| a.parse_u32())
            .filter(|&v| v >= 1)
    }
}

/// Document-absolute source position of a `Section`, in both byte offsets
/// (for tooling that needs precise slicing/patching, e.g. LSP text edits)
/// and 0-based line/column (matching `start_line`/`end_line`'s own
/// convention). Columns are always 0 today — this format is line-based
/// (section headers/content always start at column 0), so there's nothing
/// finer to report yet; the fields exist so a future per-token span (e.g. a
/// specific JSON key within a REQUEST body) has somewhere consistent to
/// plug in without another struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SectionSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl SectionSpan {
    /// Build a `SectionSpan` for the half-open 0-based line range `[start_line,
    /// end_line)`, using `line_offsets` (see `line_start_byte_offsets`) to
    /// resolve byte positions. `source_len` is the fallback for a line index
    /// past the last tracked offset (e.g. `end_line` at EOF with no trailing
    /// newline).
    pub fn from_line_range(
        line_offsets: &[usize],
        start_line: usize,
        end_line: usize,
        source_len: usize,
    ) -> Self {
        let start_byte = line_offsets.get(start_line).copied().unwrap_or(source_len);
        let end_byte = line_offsets
            .get(end_line)
            .copied()
            .unwrap_or(source_len)
            .max(start_byte);
        Self {
            start_byte,
            end_byte,
            start_line,
            start_col: 0,
            end_line,
            end_col: 0,
        }
    }
}

/// Byte offset where each 0-based line starts, for `source` — index `k`
/// gives the byte position right after the `k`-th `\n` (index 0 is always
/// 0). Only `\n` bytes count as separators, so `\r\n`-terminated lines work
/// identically to `\n`-terminated ones (a `\r` immediately before a `\n`
/// stays part of the preceding line's byte range, matching how
/// `str::lines()` treats it).
pub fn line_start_byte_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    offsets.extend(
        source
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| i + 1),
    );
    offsets
}

/// A section in the .gctf file
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Section type
    pub section_type: SectionType,

    /// Content of the section (raw text, typically JSON)
    pub content: SectionContent,

    /// Inline options (for sections that support them)
    pub inline_options: InlineOptions,

    /// Raw text content of the section (preserved for formatting)
    pub raw_content: String,

    /// Line number where section starts
    pub start_line: usize,

    /// Line number where section ends
    pub end_line: usize,

    /// Attributes declared on this section
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<GctfAttribute>,

    /// Document-absolute byte/line/col span, duplicating `start_line`/
    /// `end_line` in a richer shape — see `SectionSpan`. Defaults to
    /// all-zero (`SectionSpan::default()`) when a caller doesn't know or
    /// care about it (most tests, and any document built by hand rather than
    /// parsed from real source); real parses always populate it.
    #[serde(default)]
    pub span: SectionSpan,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            section_type: SectionType::Address,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        }
    }
}

/// Section content
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SectionContent {
    /// Single value (ADDRESS, ENDPOINT, etc.)
    Single(String),

    /// JSON object (REQUEST, RESPONSE, ERROR)
    Json(serde_json::Value),

    /// Newline-delimited JSON values within a single section block
    JsonLines(Vec<serde_json::Value>),

    /// Key-value pairs (REQUEST_HEADERS, TLS, OPTIONS, PROTO)
    KeyValues(OrderedStringMap),

    /// Extract variables from response (EXTRACT)
    Extract(OrderedStringMap),

    /// Assertion expressions (ASSERTS)
    Assertions(Vec<String>),

    /// File-level metadata (META)
    Meta(FileMeta),

    /// Inline data-driven rows (DATASET) — a YAML list of row objects, each
    /// becoming one `dataset.<field>` template expansion.
    Rows(Vec<serde_json::Value>),

    /// Empty section
    Empty,
}

/// Section types in .gctf files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SectionType {
    /// Server address
    Address,

    /// gRPC endpoint (service/method)
    Endpoint,

    /// Request payload (can have multiple)
    Request,

    /// Expected response (can have multiple)
    Response,

    /// Expected error
    Error,

    /// Request-specific headers
    RequestHeaders,

    /// Assertion expressions (can have multiple)
    Asserts,

    /// Protocol buffer configuration
    Proto,

    /// TLS/mTLS configuration
    Tls,

    /// Test execution options
    Options,

    /// Extract variables from response
    Extract,

    /// File-level metadata (suite, tags)
    Meta,

    /// File-level benchmark profile/options
    Bench,

    /// Inline data-driven test rows (YAML list of objects)
    Dataset,
}

impl SectionType {
    /// Returns `true` if this section marks the end of a logical request-response cycle.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SectionType::Response | SectionType::Error | SectionType::Asserts
        )
    }

    /// Get section name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionType::Address => "ADDRESS",
            SectionType::Endpoint => "ENDPOINT",
            SectionType::Request => "REQUEST",
            SectionType::Response => "RESPONSE",
            SectionType::Error => "ERROR",
            SectionType::RequestHeaders => "REQUEST_HEADERS",
            SectionType::Asserts => "ASSERTS",
            SectionType::Proto => "PROTO",
            SectionType::Tls => "TLS",
            SectionType::Options => "OPTIONS",
            SectionType::Extract => "EXTRACT",
            SectionType::Meta => "META",
            SectionType::Bench => "BENCH",
            SectionType::Dataset => "DATASET",
        }
    }

    /// Parse section name string to SectionType
    pub fn from_keyword(s: &str) -> Option<SectionType> {
        match s.trim() {
            "ADDRESS" => Some(SectionType::Address),
            "ENDPOINT" => Some(SectionType::Endpoint),
            "REQUEST" => Some(SectionType::Request),
            "RESPONSE" => Some(SectionType::Response),
            "ERROR" => Some(SectionType::Error),
            "REQUEST_HEADERS" | "HEADERS" => Some(SectionType::RequestHeaders),
            "ASSERTS" => Some(SectionType::Asserts),
            "PROTO" => Some(SectionType::Proto),
            "TLS" => Some(SectionType::Tls),
            "OPTIONS" => Some(SectionType::Options),
            "EXTRACT" => Some(SectionType::Extract),
            "META" => Some(SectionType::Meta),
            "BENCH" => Some(SectionType::Bench),
            "DATASET" => Some(SectionType::Dataset),
            _ => None,
        }
    }

    /// Check if section can appear multiple times
    #[must_use]
    pub fn is_multiple_allowed(&self) -> bool {
        matches!(
            self,
            SectionType::Request
                | SectionType::Response
                | SectionType::Asserts
                | SectionType::Extract
        )
    }

    /// Check if section is file-level (not inside documents)
    #[must_use]
    pub fn is_file_level(&self) -> bool {
        matches!(self, SectionType::Meta | SectionType::Bench)
    }

    pub fn supports_inline_options(&self) -> bool {
        matches!(self, SectionType::Response | SectionType::Error)
    }

    pub fn preamble_rank(&self) -> Option<usize> {
        match self {
            SectionType::Meta => Some(0),
            SectionType::Bench => Some(1),
            // DATASET is file-level test configuration like BENCH, not a
            // connection detail — and its `dataset.*` fields are referenced
            // via `{{dataset.field}}` inside REQUEST, so it must read before
            // REQUEST rather than sitting wherever it happened to be typed.
            SectionType::Dataset => Some(2),
            SectionType::Address => Some(3),
            SectionType::Endpoint => Some(4),
            SectionType::Tls => Some(5),
            SectionType::Proto => Some(6),
            SectionType::Options => Some(7),
            _ => None,
        }
    }
}

/// Inline options for sections
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InlineOptions {
    /// Run ASSERTS on same response (unary RPC)
    pub with_asserts: bool,

    /// Subset comparison (expected is subset of actual)
    pub partial: bool,

    /// Numeric tolerance for floating-point comparisons
    pub tolerance: Option<f64>,

    /// Remove sensitive fields before comparison
    pub redact: Vec<String>,

    /// Sort arrays for order-independent comparison
    pub unordered_arrays: bool,

    /// Plugin-declared inline-option keys (`@inline_option` doc tag) and
    /// their raw string values — the parser accepts any key a loaded plugin
    /// has registered instead of hard-rejecting it as unknown. Ordered for
    /// deterministic `fmt` round-tripping.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: std::collections::BTreeMap<String, String>,
}

impl InlineOptions {
    pub fn to_header_tokens(&self) -> Vec<String> {
        let mut parts = Vec::new();

        if self.partial {
            parts.push("partial".to_string());
        }

        if let Some(tolerance) = self.tolerance {
            parts.push(format!("tolerance={}", tolerance));
        }

        if !self.redact.is_empty() {
            let mut sorted_redact = self.redact.clone();
            sorted_redact.sort();
            let quoted = sorted_redact
                .iter()
                .map(|field| format!("\"{}\"", field))
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("redact=[{}]", quoted));
        }

        if self.unordered_arrays {
            parts.push("unordered_arrays".to_string());
        }

        if self.with_asserts {
            parts.push("with_asserts".to_string());
        }

        for (key, value) in &self.extra {
            parts.push(format!("{key}={value}"));
        }

        parts
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.with_asserts
            && !self.partial
            && self.tolerance.is_none()
            && self.redact.is_empty()
            && !self.unordered_arrays
            && self.extra.is_empty()
    }
}

impl Section {
    pub fn format_header(&self) -> String {
        let section = self.section_type.as_str();
        if self.section_type.supports_inline_options() {
            let parts = self.inline_options.to_header_tokens();
            if parts.is_empty() {
                format!("--- {} ---", section)
            } else {
                format!("--- {} {} ---", section, parts.join(" "))
            }
        } else {
            format!("--- {} ---", section)
        }
    }

    pub fn header_keyword_from_source<'a>(&self, source: &'a str) -> Option<&'a str> {
        let header_line = source.lines().nth(self.start_line)?.trim();
        let inner = header_line.strip_prefix("---")?.strip_suffix("---")?.trim();
        inner.split_whitespace().next()
    }
}

/// GCTF file header with inline options
/// Format: --- SECTION_NAME key=value ... ---
#[derive(Debug, Clone, PartialEq)]
pub struct SectionHeader {
    /// Section type
    pub section_type: SectionType,

    /// Inline options (key=value pairs)
    pub options: HashMap<String, String>,
}

impl GctfDocument {
    /// Create a new empty document
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            sections: Vec::new(),
            metadata: DocumentMetadata::default(),
            next_document: None,
        }
    }

    /// Get document by index (0-based) from the chain
    pub fn get_document(&self, index: usize) -> Option<&GctfDocument> {
        self.iter_chain().nth(index)
    }

    pub fn iter_chain(&self) -> DocumentChainIter<'_> {
        DocumentChainIter {
            current: Some(self),
        }
    }

    pub fn document_count(&self) -> usize {
        let mut count = 1;
        let mut current = &self.next_document;
        while let Some(doc) = current {
            count += 1;
            current = &doc.next_document;
        }
        count
    }

    /// Clone with `next_document` cleared — for passing one chain member to
    /// a chain-aware function without it also walking the rest of the chain.
    #[must_use]
    pub fn detached(&self) -> GctfDocument {
        GctfDocument {
            next_document: None,
            ..self.clone()
        }
    }

    #[must_use]
    pub fn is_single_document(&self) -> bool {
        self.next_document.is_none()
    }

    /// Get all sections of a specific type
    pub fn sections_by_type(&self, section_type: SectionType) -> Vec<&Section> {
        self.sections
            .iter()
            .filter(|s| s.section_type == section_type)
            .collect()
    }

    /// Get first section of a specific type
    pub fn first_section(&self, section_type: SectionType) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.section_type == section_type)
    }

    /// Get address (from ADDRESS section or environment variable)
    pub fn get_address(&self, env_address: Option<&str>) -> Option<String> {
        if let Some(section) = self.first_section(SectionType::Address)
            && let SectionContent::Single(addr) = &section.content
        {
            return Some(addr.clone());
        }
        env_address.map(|s| s.to_string())
    }

    /// Get endpoint
    pub fn get_endpoint(&self) -> Option<String> {
        if let Some(section) = self.first_section(SectionType::Endpoint)
            && let SectionContent::Single(endpoint) = &section.content
        {
            return Some(endpoint.clone());
        }
        None
    }

    /// Parse endpoint into package, service, method
    pub fn parse_endpoint(&self) -> Option<(String, String, String)> {
        let endpoint = self.get_endpoint()?;
        let parts: Vec<&str> = endpoint.split('/').collect();
        if parts.len() == 2 {
            let full_service = parts[0];
            let service_parts: Vec<&str> = full_service.split('.').collect();
            if service_parts.len() >= 2 {
                let package = service_parts[..service_parts.len() - 1].join(".");
                let service = service_parts[service_parts.len() - 1].to_string();
                let method = parts[1].to_string();
                return Some((package, service, method));
            } else if service_parts.len() == 1 {
                let package = String::new();
                let service = service_parts[0].to_string();
                let method = parts[1].to_string();
                return Some((package, service, method));
            }
        }
        None
    }

    /// Get all request payloads
    pub fn get_requests(&self) -> Vec<serde_json::Value> {
        self.sections_by_type(SectionType::Request)
            .into_iter()
            .filter_map(|s| {
                if let SectionContent::Json(json) = &s.content {
                    Some(json.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all assertion sections
    pub fn get_assertions(&self) -> Vec<Vec<String>> {
        self.sections_by_type(SectionType::Asserts)
            .into_iter()
            .filter_map(|s| {
                if let SectionContent::Assertions(asserts) = &s.content {
                    Some(asserts.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get request headers
    pub fn get_request_headers(&self) -> Option<OrderedStringMap> {
        if let Some(section) = self.first_section(SectionType::RequestHeaders)
            && let SectionContent::KeyValues(headers) = &section.content
        {
            return Some(headers.clone());
        }
        None
    }

    /// Get TLS configuration
    pub fn get_tls_config(&self) -> Option<OrderedStringMap> {
        if let Some(section) = self.first_section(SectionType::Tls)
            && let SectionContent::KeyValues(config) = &section.content
        {
            return Some(config.clone());
        }
        None
    }

    /// Get OPTIONS configuration
    pub fn get_options(&self) -> Option<OrderedStringMap> {
        if let Some(section) = self.first_section(SectionType::Options)
            && let SectionContent::KeyValues(config) = &section.content
        {
            return Some(config.clone());
        }
        None
    }

    /// Get TLS configuration merged with defaults (section values override defaults)
    pub fn get_tls_config_with_defaults(
        &self,
        defaults: &OrderedStringMap,
    ) -> Option<OrderedStringMap> {
        let mut merged = defaults.clone();

        if let Some(section) = self.first_section(SectionType::Tls)
            && let SectionContent::KeyValues(config) = &section.content
        {
            for (key, value) in config {
                merged.insert(key.clone(), value.clone());
            }
        }

        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    }

    /// Get PROTO configuration
    pub fn get_proto_config(&self) -> Option<OrderedStringMap> {
        if let Some(section) = self.first_section(SectionType::Proto)
            && let SectionContent::KeyValues(config) = &section.content
        {
            return Some(config.clone());
        }
        None
    }

    /// Check for RESPONSE and ERROR conflict
    #[must_use]
    pub fn has_response_error_conflict(&self) -> bool {
        self.first_section(SectionType::Response).is_some()
            && self.first_section(SectionType::Error).is_some()
    }

    pub fn section_uses_deprecated_headers_alias(&self, section: &Section) -> bool {
        if section.section_type != SectionType::RequestHeaders {
            return false;
        }

        self.metadata
            .source
            .as_deref()
            .and_then(|source| section.header_keyword_from_source(source))
            .is_some_and(|keyword| keyword.eq_ignore_ascii_case("HEADERS"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_line_start_byte_offsets_lf() {
        let source = "abc\nde\nfghi";
        // line 0: "abc" (0..3), line 1: "de" (4..6), line 2: "fghi" (7..11)
        assert_eq!(line_start_byte_offsets(source), vec![0, 4, 7]);
    }

    #[test]
    fn test_line_start_byte_offsets_crlf() {
        // `\r` stays part of the preceding line's byte range — only `\n`
        // advances to the next line's start.
        let source = "ab\r\ncd";
        assert_eq!(line_start_byte_offsets(source), vec![0, 4]);
    }

    #[test]
    fn test_line_start_byte_offsets_empty() {
        assert_eq!(line_start_byte_offsets(""), vec![0]);
    }

    #[test]
    fn test_section_span_from_line_range() {
        let source = "abc\ndefg\nh\n";
        let offsets = line_start_byte_offsets(source);
        // Section spanning lines [1, 2) — just "defg".
        let span = SectionSpan::from_line_range(&offsets, 1, 2, source.len());
        assert_eq!(span.start_byte, 4);
        assert_eq!(span.end_byte, 9);
        assert_eq!(&source[span.start_byte..span.end_byte], "defg\n");
        assert_eq!(span.start_line, 1);
        assert_eq!(span.end_line, 2);
    }

    #[test]
    fn test_section_span_from_line_range_past_eof_falls_back_to_source_len() {
        let source = "only one line, no trailing newline";
        let offsets = line_start_byte_offsets(source);
        let span = SectionSpan::from_line_range(&offsets, 0, 1, source.len());
        assert_eq!(span.start_byte, 0);
        assert_eq!(span.end_byte, source.len());
    }

    #[test]
    fn test_section_type_from_str() {
        assert_eq!(
            SectionType::from_keyword("ADDRESS"),
            Some(SectionType::Address)
        );
        assert_eq!(
            SectionType::from_keyword("ENDPOINT"),
            Some(SectionType::Endpoint)
        );
        assert_eq!(SectionType::from_keyword("INVALID"), None);
    }

    #[test]
    fn test_section_type_multiple_allowed() {
        assert!(SectionType::Request.is_multiple_allowed());
        assert!(SectionType::Response.is_multiple_allowed());
        assert!(SectionType::Asserts.is_multiple_allowed());
        assert!(!SectionType::Address.is_multiple_allowed());
        assert!(!SectionType::Endpoint.is_multiple_allowed());
    }

    #[test]
    fn test_section_type_supports_inline_options() {
        assert!(SectionType::Response.supports_inline_options());
        assert!(SectionType::Error.supports_inline_options());
        assert!(!SectionType::Request.supports_inline_options());
        assert!(!SectionType::Address.supports_inline_options());
    }

    #[test]
    fn test_section_type_as_str() {
        assert_eq!(SectionType::Address.as_str(), "ADDRESS");
        assert_eq!(SectionType::Endpoint.as_str(), "ENDPOINT");
        assert_eq!(SectionType::Request.as_str(), "REQUEST");
        assert_eq!(SectionType::Response.as_str(), "RESPONSE");
        assert_eq!(SectionType::Error.as_str(), "ERROR");
        assert_eq!(SectionType::RequestHeaders.as_str(), "REQUEST_HEADERS");
        assert_eq!(SectionType::Asserts.as_str(), "ASSERTS");
        assert_eq!(SectionType::Proto.as_str(), "PROTO");
        assert_eq!(SectionType::Tls.as_str(), "TLS");
        assert_eq!(SectionType::Options.as_str(), "OPTIONS");
        assert_eq!(SectionType::Extract.as_str(), "EXTRACT");
        assert_eq!(SectionType::Meta.as_str(), "META");
        assert_eq!(SectionType::Bench.as_str(), "BENCH");
        assert_eq!(SectionType::Dataset.as_str(), "DATASET");
    }

    #[test]
    fn test_dataset_round_trips_through_keyword_and_preamble_rank() {
        assert_eq!(
            SectionType::from_keyword("DATASET"),
            Some(SectionType::Dataset)
        );
        assert_eq!(SectionType::Dataset.as_str(), "DATASET");
        // File-level configuration like BENCH, not a connection detail —
        // must sort before ADDRESS/ENDPOINT/TLS/PROTO/OPTIONS since its
        // fields are referenced via `{{dataset.field}}` inside REQUEST.
        assert!(SectionType::Dataset.preamble_rank() > SectionType::Bench.preamble_rank());
        assert!(SectionType::Dataset.preamble_rank() < SectionType::Address.preamble_rank());
        assert!(!SectionType::Dataset.is_multiple_allowed());
        assert!(!SectionType::Dataset.is_terminal());
    }

    #[test]
    fn test_section_type_from_keyword_aliases() {
        assert_eq!(
            SectionType::from_keyword("HEADERS"),
            Some(SectionType::RequestHeaders)
        );
        assert_eq!(
            SectionType::from_keyword("REQUEST_HEADERS"),
            Some(SectionType::RequestHeaders)
        );
        assert_eq!(SectionType::from_keyword("BENCH"), Some(SectionType::Bench));
    }

    #[test]
    fn test_section_type_from_keyword_case_insensitive() {
        // Should be case sensitive based on implementation
        assert_eq!(SectionType::from_keyword("address"), None);
        assert_eq!(
            SectionType::from_keyword("  ADDRESS  "),
            Some(SectionType::Address)
        );
    }

    #[test]
    fn test_section_type_is_terminal() {
        assert!(SectionType::Response.is_terminal());
        assert!(SectionType::Error.is_terminal());
        assert!(SectionType::Asserts.is_terminal());
        assert!(!SectionType::Request.is_terminal());
        assert!(!SectionType::Endpoint.is_terminal());
        assert!(!SectionType::Extract.is_terminal());
        assert!(!SectionType::Address.is_terminal());
    }

    #[test]
    fn test_gctf_document_new() {
        let doc = GctfDocument::new("test.gctf".to_string());
        assert_eq!(doc.file_path, "test.gctf");
        assert!(doc.sections.is_empty());
        assert!(doc.metadata.source.is_none());
        assert!(doc.metadata.mtime.is_none());
    }

    #[test]
    fn test_gctf_document_sections_by_type() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value1"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value2"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 5,
            end_line: 6,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let requests = doc.sections_by_type(SectionType::Request);
        assert_eq!(requests.len(), 2);

        let responses = doc.sections_by_type(SectionType::Response);
        assert_eq!(responses.len(), 1);

        let errors = doc.sections_by_type(SectionType::Error);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_gctf_document_first_section() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let first_request = doc.first_section(SectionType::Request);
        assert!(first_request.is_some());

        let first_error = doc.first_section(SectionType::Error);
        assert!(first_error.is_none());
    }

    #[test]
    fn test_gctf_document_get_address() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Address,
            content: SectionContent::Single("localhost:4770".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        assert_eq!(doc.get_address(None), Some("localhost:4770".to_string()));
        assert_eq!(
            doc.get_address(Some("env:5000")),
            Some("localhost:4770".to_string())
        );

        let doc2 = GctfDocument::new("test.gctf".to_string());
        assert_eq!(
            doc2.get_address(Some("env:5000")),
            Some("env:5000".to_string())
        );
        assert_eq!(doc2.get_address(None), None);
    }

    #[test]
    fn test_gctf_document_get_endpoint() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("my.Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        assert_eq!(doc.get_endpoint(), Some("my.Service/Method".to_string()));

        let doc2 = GctfDocument::new("test.gctf".to_string());
        assert_eq!(doc2.get_endpoint(), None);
    }

    #[test]
    fn test_gctf_document_parse_endpoint() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("package.Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let (package, service, method) = doc.parse_endpoint().unwrap();
        assert_eq!(package, "package");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[test]
    fn test_gctf_document_parse_endpoint_no_package() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("Service/Method".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let (package, service, method) = doc.parse_endpoint().unwrap();
        assert_eq!(package, "");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[test]
    fn test_gctf_document_parse_endpoint_invalid() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Endpoint,
            content: SectionContent::Single("invalid".to_string()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 1,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        assert!(doc.parse_endpoint().is_none());
    }

    #[test]
    fn test_gctf_document_get_requests() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value1"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Request,
            content: SectionContent::Json(json!({"key": "value2"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let requests = doc.get_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], json!({"key": "value1"}));
        assert_eq!(requests[1], json!({"key": "value2"}));
    }

    #[test]
    fn test_gctf_document_get_assertions() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".id == 1".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        doc.sections.push(Section {
            section_type: SectionType::Asserts,
            content: SectionContent::Assertions(vec![".name == \"test\"".to_string()]),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let assertions = doc.get_assertions();
        assert_eq!(assertions.len(), 2);
        assert_eq!(assertions[0], vec![".id == 1"]);
        assert_eq!(assertions[1], vec![".name == \"test\""]);
    }

    #[test]
    fn test_gctf_document_get_request_headers() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut headers = OrderedStringMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        doc.sections.push(Section {
            section_type: SectionType::RequestHeaders,
            content: SectionContent::KeyValues(headers.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = doc.get_request_headers().unwrap();
        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_gctf_document_get_tls_config() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut config = OrderedStringMap::new();
        config.insert("ca_cert".to_string(), "/path/to/ca.pem".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Tls,
            content: SectionContent::KeyValues(config.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = doc.get_tls_config().unwrap();
        assert_eq!(result.get("ca_cert"), Some(&"/path/to/ca.pem".to_string()));
    }

    #[test]
    fn test_gctf_document_get_options() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut options = OrderedStringMap::new();
        options.insert("dry_run".to_string(), "true".to_string());
        options.insert("timeout".to_string(), "10".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Options,
            content: SectionContent::KeyValues(options.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = doc.get_options().unwrap();
        assert_eq!(result.get("dry_run"), Some(&"true".to_string()));
        assert_eq!(result.get("timeout"), Some(&"10".to_string()));
    }

    #[test]
    fn test_gctf_document_get_tls_config_with_defaults_env_only() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let mut defaults = OrderedStringMap::new();
        defaults.insert("server_name".to_string(), "example.com".to_string());

        let result = doc.get_tls_config_with_defaults(&defaults).unwrap();
        assert_eq!(result.get("server_name"), Some(&"example.com".to_string()));
    }

    #[test]
    fn test_gctf_document_get_tls_config_with_defaults_section_overrides() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut config = OrderedStringMap::new();
        config.insert("insecure".to_string(), "true".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Tls,
            content: SectionContent::KeyValues(config),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let mut defaults = OrderedStringMap::new();
        defaults.insert("insecure".to_string(), "false".to_string());
        defaults.insert("server_name".to_string(), "example.com".to_string());

        let result = doc.get_tls_config_with_defaults(&defaults).unwrap();
        assert_eq!(result.get("insecure"), Some(&"true".to_string()));
        assert_eq!(result.get("server_name"), Some(&"example.com".to_string()));
    }

    #[test]
    fn test_gctf_document_get_proto_config() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        let mut config = OrderedStringMap::new();
        config.insert("files".to_string(), "service.proto".to_string());
        doc.sections.push(Section {
            section_type: SectionType::Proto,
            content: SectionContent::KeyValues(config.clone()),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        let result = doc.get_proto_config().unwrap();
        assert_eq!(result.get("files"), Some(&"service.proto".to_string()));
    }

    #[test]
    fn test_gctf_document_has_response_error_conflict() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        assert!(!doc.has_response_error_conflict());

        doc.sections.push(Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(json!({"result": "ok"})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 1,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        assert!(!doc.has_response_error_conflict());

        doc.sections.push(Section {
            section_type: SectionType::Error,
            content: SectionContent::Json(json!({"code": 5})),
            inline_options: InlineOptions::default(),
            raw_content: "".to_string(),
            start_line: 3,
            end_line: 4,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });
        assert!(doc.has_response_error_conflict());
    }

    #[test]
    fn test_inline_options_default() {
        let options = InlineOptions::default();
        assert!(!options.with_asserts);
        assert!(!options.partial);
        assert!(options.tolerance.is_none());
        assert!(options.redact.is_empty());
        assert!(!options.unordered_arrays);
    }

    #[test]
    fn test_section_format_header_with_inline_options() {
        let section = Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"ok": true})),
            inline_options: InlineOptions {
                with_asserts: true,
                partial: true,
                tolerance: Some(0.1),
                redact: vec!["token".to_string()],
                unordered_arrays: true,
                ..InlineOptions::default()
            },
            raw_content: "".to_string(),
            start_line: 0,
            end_line: 0,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let header = section.format_header();
        assert_eq!(
            header,
            "--- RESPONSE partial tolerance=0.1 redact=[\"token\"] unordered_arrays with_asserts ---"
        );
    }

    #[test]
    fn test_inline_options_extra_round_trips_and_counts_toward_is_empty() {
        let mut options = InlineOptions::default();
        assert!(options.is_empty());

        options
            .extra
            .insert("priority".to_string(), "urgent".to_string());
        assert!(!options.is_empty());
        assert_eq!(options.to_header_tokens(), vec!["priority=urgent"]);
    }

    #[test]
    fn test_section_content_debug() {
        let content = SectionContent::Single("test".to_string());
        let debug_str = format!("{:?}", content);
        assert!(debug_str.contains("Single"));
    }

    #[test]
    fn test_section_header_keyword_from_source() {
        let section = Section {
            section_type: SectionType::Response,
            content: SectionContent::Json(serde_json::json!({"ok": true})),
            inline_options: InlineOptions::default(),
            raw_content: "{\"ok\":true}".to_string(),
            start_line: 0,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        };

        let source = "--- RESPONSE with_asserts=true ---\n{\"ok\":true}\n";
        assert_eq!(section.header_keyword_from_source(source), Some("RESPONSE"));
    }

    #[test]
    fn test_document_detects_deprecated_headers_alias() {
        let mut doc = GctfDocument::new("test.gctf".to_string());
        doc.metadata.source = Some("--- HEADERS ---\nAuthorization: Bearer t\n".to_string());
        doc.sections.push(Section {
            section_type: SectionType::RequestHeaders,
            content: SectionContent::KeyValues(OrderedStringMap::from([(
                "Authorization".to_string(),
                "Bearer t".to_string(),
            )])),
            inline_options: InlineOptions::default(),
            raw_content: "Authorization: Bearer t".to_string(),
            start_line: 0,
            end_line: 2,
            attributes: Vec::new(),
            span: SectionSpan::default(),
        });

        assert!(doc.section_uses_deprecated_headers_alias(&doc.sections[0]));
    }

    #[test]
    fn test_gctf_document_debug() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let debug_str = format!("{:?}", doc);
        assert!(debug_str.contains("test.gctf"));
    }

    #[test]
    fn test_document_chain_single() {
        let doc = GctfDocument::new("test.gctf".to_string());
        assert!(doc.is_single_document());
        assert_eq!(doc.document_count(), 1);
    }

    #[test]
    fn test_document_chain_two_docs() {
        let mut doc1 = GctfDocument::new("test.gctf".to_string());
        let doc2 = GctfDocument::new("test.gctf".to_string());
        doc1.next_document = Some(Box::new(doc2));

        assert!(!doc1.is_single_document());
        assert_eq!(doc1.document_count(), 2);
    }

    #[test]
    fn test_document_chain_three_docs() {
        let mut doc3 = GctfDocument::new("test.gctf".to_string());
        doc3.file_path = "doc3".to_string();

        let mut doc2 = GctfDocument::new("test.gctf".to_string());
        doc2.file_path = "doc2".to_string();
        doc2.next_document = Some(Box::new(doc3));

        let mut doc1 = GctfDocument::new("test.gctf".to_string());
        doc1.file_path = "doc1".to_string();
        doc1.next_document = Some(Box::new(doc2));

        assert_eq!(doc1.document_count(), 3);

        let docs: Vec<_> = doc1.iter_chain().collect();
        assert_eq!(docs.len(), 3);
        assert_eq!(docs[0].file_path, "doc1");
        assert_eq!(docs[1].file_path, "doc2");
        assert_eq!(docs[2].file_path, "doc3");
    }

    #[test]
    fn test_document_chain_get_document() {
        let mut doc2 = GctfDocument::new("test.gctf".to_string());
        doc2.file_path = "doc2".to_string();

        let mut doc1 = GctfDocument::new("test.gctf".to_string());
        doc1.file_path = "doc1".to_string();
        doc1.next_document = Some(Box::new(doc2));

        assert_eq!(doc1.get_document(0).unwrap().file_path, "doc1");
        assert_eq!(doc1.get_document(1).unwrap().file_path, "doc2");
        assert!(doc1.get_document(2).is_none());
    }

    #[test]
    fn test_document_chain_iter_on_last() {
        let doc = GctfDocument::new("test.gctf".to_string());
        let docs: Vec<_> = doc.iter_chain().collect();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].file_path, "test.gctf");
    }

    #[test]
    fn test_gctf_attribute_parse_u64() {
        assert_eq!(GctfAttribute::new("timeout", "30").parse_u64(), Some(30));
        assert_eq!(
            GctfAttribute::new("timeout", "  30  ").parse_u64(),
            Some(30)
        );
        assert_eq!(GctfAttribute::new("timeout", "0").parse_u64(), Some(0));
        assert_eq!(GctfAttribute::new("timeout", "abc").parse_u64(), None);
        assert_eq!(GctfAttribute::new("timeout", "-1").parse_u64(), None);
    }

    #[test]
    fn test_gctf_attribute_parse_u32() {
        assert_eq!(GctfAttribute::new("retry", "3").parse_u32(), Some(3));
        assert_eq!(GctfAttribute::new("retry", "  5  ").parse_u32(), Some(5));
        assert_eq!(GctfAttribute::new("retry", "0").parse_u32(), Some(0));
        assert_eq!(GctfAttribute::new("retry", "abc").parse_u32(), None);
    }

    #[test]
    fn test_gctf_attribute_parse_f64() {
        assert_eq!(
            GctfAttribute::new("tolerance", "0.1").parse_f64(),
            Some(0.1)
        );
        assert_eq!(
            GctfAttribute::new("tolerance", "  1.5  ").parse_f64(),
            Some(1.5)
        );
        assert_eq!(GctfAttribute::new("tolerance", "abc").parse_f64(), None);
    }

    #[test]
    fn test_gctf_attribute_parse_bool() {
        let cases_true = vec!["true", "1", "yes", "on", "True", "TRUE", "YES"];
        for v in cases_true {
            assert_eq!(
                GctfAttribute::new("skip", v).parse_bool(),
                Some(true),
                "failed for {}",
                v
            );
        }
        let cases_false = vec!["false", "0", "no", "off", "", "False", "FALSE"];
        for v in cases_false {
            assert_eq!(
                GctfAttribute::new("skip", v).parse_bool(),
                Some(false),
                "failed for {}",
                v
            );
        }
        assert_eq!(GctfAttribute::new("skip", "maybe").parse_bool(), None);
    }

    #[test]
    fn test_gctf_attribute_as_str() {
        assert_eq!(GctfAttribute::new("name", "hello").as_str(), "hello");
        assert_eq!(GctfAttribute::flag("skip").as_str(), "true");
    }

    #[test]
    fn test_canonical_key_spelling() {
        assert_eq!(canonical_key_spelling("retry-delay"), "retry_delay");
        assert_eq!(canonical_key_spelling("no-retry"), "no_retry");
        // Already-canonical and unrelated keys pass through unchanged.
        assert_eq!(canonical_key_spelling("retry_delay"), "retry_delay");
        assert_eq!(canonical_key_spelling("timeout"), "timeout");
    }

    #[test]
    fn test_gctf_attribute_format_directive_canonicalizes_deprecated_names() {
        assert_eq!(
            GctfAttribute::new("retry-delay", "0.5").format_directive(),
            "#[retry_delay(0.5)]"
        );
        assert_eq!(
            GctfAttribute::flag("no-retry").format_directive(),
            "#[no_retry]"
        );
        // Already-canonical names are untouched.
        assert_eq!(
            GctfAttribute::new("timeout", "5").format_directive(),
            "#[timeout(5)]"
        );
    }

    #[test]
    fn test_section_get_attribute() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![
                GctfAttribute::new("timeout", "30"),
                GctfAttribute::new("retry", "2"),
            ],
            span: SectionSpan::default(),
        };
        assert!(section.get_attribute("timeout").is_some());
        assert!(section.get_attribute("retry").is_some());
        assert!(section.get_attribute("skip").is_none());
        assert_eq!(
            section.get_attribute("timeout").unwrap().parse_u64(),
            Some(30)
        );
    }

    #[test]
    fn test_section_get_timeout() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("timeout", "10")],
            span: SectionSpan::default(),
        };
        assert_eq!(section.get_timeout(), Some(10));
    }

    #[test]
    fn test_section_get_timeout_zero() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("timeout", "0")],
            span: SectionSpan::default(),
        };
        assert_eq!(section.get_timeout(), None);
    }

    #[test]
    fn test_section_get_timeout_missing() {
        let section = Section::default();
        assert_eq!(section.get_timeout(), None);
    }

    #[test]
    fn test_section_get_retry() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("retry", "3")],
            span: SectionSpan::default(),
        };
        assert_eq!(section.get_retry(), Some(3));
    }

    #[test]
    fn test_section_get_retry_missing() {
        let section = Section::default();
        assert_eq!(section.get_retry(), None);
    }

    #[test]
    fn test_section_get_skip() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::flag("skip")],
            span: SectionSpan::default(),
        };
        assert!(section.get_skip());
    }

    #[test]
    fn test_section_get_skip_explicit() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("skip", "true")],
            span: SectionSpan::default(),
        };
        assert!(section.get_skip());
    }

    #[test]
    fn test_section_get_skip_false() {
        let section = Section::default();
        assert!(!section.get_skip());
    }

    #[test]
    fn test_section_has_tag() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("tag", "smoke,slow")],
            span: SectionSpan::default(),
        };
        assert!(section.has_tag("smoke"));
        assert!(section.has_tag("slow"));
        assert!(!section.has_tag("integration"));
    }

    #[test]
    fn test_section_has_tag_single() {
        let section = Section {
            section_type: SectionType::Request,
            content: SectionContent::Empty,
            inline_options: InlineOptions::default(),
            raw_content: String::new(),
            start_line: 0,
            end_line: 0,
            attributes: vec![GctfAttribute::new("tag", "smoke")],
            span: SectionSpan::default(),
        };
        assert!(section.has_tag("smoke"));
        assert!(!section.has_tag("slow"));
    }

    #[test]
    fn test_section_has_tag_missing() {
        let section = Section::default();
        assert!(!section.has_tag("smoke"));
    }
}
