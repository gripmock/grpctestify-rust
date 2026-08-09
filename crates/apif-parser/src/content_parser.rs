#![allow(clippy::unwrap_used, clippy::expect_used)] // audited safe
//! Section content parser for GCTF files.
//!
//! Parses the content of different section types based on their structure.

use anyhow::Result;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::assertions::strip_assertion_comments;
use crate::ast::{FileMeta, GctfAttribute, InlineOptions, Section, SectionContent, SectionType};
use crate::gctf_tokenizer::{
    strip_gctf_comment_lines, tokenize_extract_line, tokenize_inline_options, tokenize_kv_line,
};
use crate::json_mod;
use crate::json_stream_parser;

/// Inline-option keys a loaded `.rhai` plugin has declared via `@inline_option`
/// (`apif_plugins::rhai_plugin::load_all_inline_option_keys`) — set once at
/// process/command startup, same `OnceLock`-set-once-early pattern as
/// `apif_optimizer::register_extra_boolean_plugins` and
/// `apif_semantics::register_extra_plugin_names`. Never set (e.g. a bare
/// library use of this crate) means no plugin-provided keys, not a panic.
static EXTRA_INLINE_OPTION_KEYS: OnceLock<HashSet<String>> = OnceLock::new();

/// Register plugin-declared inline-option keys. Call once, early, before
/// parsing any `.gctf` file — a plugin loaded mid-session needs a restart to
/// take effect, same caveat as the sibling registries.
pub fn register_extra_inline_option_keys(keys: HashSet<String>) {
    let _ = EXTRA_INLINE_OPTION_KEYS.set(keys);
}

pub(crate) fn is_extra_inline_option_key(key: &str) -> bool {
    EXTRA_INLINE_OPTION_KEYS
        .get()
        .is_some_and(|keys| keys.contains(key))
}

/// Parse section content based on section type.
pub fn parse_section_content(section_type: SectionType, content: &str) -> Result<SectionContent> {
    let content = content.trim();

    if content.is_empty() {
        return Ok(SectionContent::Empty);
    }

    match section_type {
        // Single value sections. A `//`/`#` line here is a comment, not part
        // of the value — without stripping it, `--- ADDRESS ---` preceded by a
        // comment resolved to the comment text and `check` still passed.
        SectionType::Address | SectionType::Endpoint => {
            let stripped = strip_gctf_comment_lines(content);
            let stripped = stripped.trim();
            if stripped.is_empty() {
                return Ok(SectionContent::Empty);
            }
            Ok(SectionContent::Single(stripped.to_string()))
        }

        // JSON sections
        SectionType::Request => {
            // Primary mode: a single JSON/JSON5 value (unary request).
            if let Ok(json_value) = json_mod::from_str(content) {
                return Ok(SectionContent::Json(json_value));
            }

            // Streaming mode: multiple self-delimiting JSON payloads in one
            // REQUEST block — the client/bidi-streaming counterpart to
            // RESPONSE's own JsonLines mode, additive and no-divider/no-threshold
            // for the same reason (see `json_stream_parser`).
            if let Some(values) = json_stream_parser::parse_response_json_values(content) {
                Ok(SectionContent::JsonLines(values))
            } else {
                // Preserve original parse error behavior for malformed single-value requests.
                let json_value = json_mod::from_str(content)?;
                Ok(SectionContent::Json(json_value))
            }
        }
        SectionType::Error => {
            let json_value = json_mod::from_str(content)?;
            Ok(SectionContent::Json(json_value))
        }
        SectionType::Response => {
            // Primary mode: a single JSON/JSON5 value
            if let Ok(json_value) = json_mod::from_str(content) {
                return Ok(SectionContent::Json(json_value));
            }

            // Streaming mode: multiple JSON payloads within one RESPONSE block
            if let Some(values) = json_stream_parser::parse_response_json_values(content) {
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
        SectionType::Bench => {
            let key_values = parse_bench_section(content)?;
            Ok(SectionContent::KeyValues(key_values))
        }

        // Extract section - support ternary expressions via AST
        SectionType::Extract => {
            let mut key_values = crate::ast::OrderedStringMap::new();
            for line in content.lines() {
                if let Some((name, value)) = tokenize_extract_line(line)
                    && let Some(extract_var) =
                        crate::ternary_ast::ExtractVar::parse_raw(&name, &value)
                {
                    if key_values.contains_key(&extract_var.name) {
                        anyhow::bail!(
                            "duplicate EXTRACT variable '{}' — each variable name may only be assigned once (the second occurrence would silently win)",
                            extract_var.name
                        );
                    }
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

        // META section - parse as YAML (comments allowed). A malformed or
        // unknown-field META is a hard parse error, not a silent default —
        // same "fail loud" rule as DATASET below. GCTF `//` comment lines are
        // stripped first (YAML only understands `#`) so the strict path
        // accepts the same comment styles every other context does.
        SectionType::Meta => {
            let cleaned = crate::gctf_tokenizer::strip_gctf_comment_lines(content);
            let meta = serde_yaml_ng::from_str::<FileMeta>(&cleaned)
                .map_err(|e| anyhow::anyhow!("Invalid META: {e}"))?;
            Ok(SectionContent::Meta(meta))
        }

        // DATASET section - a YAML list of row objects, each becoming one
        // `dataset.<field>` template expansion. Unlike META, a malformed
        // DATASET is a hard parse error rather than silently defaulting to
        // empty — it drives test execution, so a typo here should fail loud
        // and early rather than quietly running zero rows. GCTF `//` comment
        // lines are stripped first (see META above).
        SectionType::Dataset => {
            let cleaned = crate::gctf_tokenizer::strip_gctf_comment_lines(content);
            let rows: Vec<serde_json::Value> = serde_yaml_ng::from_str(&cleaned)
                .map_err(|e| anyhow::anyhow!("DATASET must be a YAML list of row objects: {e}"))?;
            for (i, row) in rows.iter().enumerate() {
                if !row.is_object() {
                    anyhow::bail!("DATASET row {i} must be an object, got: {row}");
                }
            }
            Ok(SectionContent::Rows(rows))
        }
    }
}

/// Build a section from parsed content.
pub fn build_section(
    section_type: SectionType,
    start_line: usize,
    end_line: usize,
    content: &[String],
    inline_options: InlineOptions,
    attributes: Vec<GctfAttribute>,
) -> Result<Section> {
    let raw_content = content.join("\n");
    let section_content = parse_section_content(section_type, &raw_content)?;

    Ok(Section {
        section_type,
        content: section_content,
        inline_options,
        raw_content,
        start_line,
        end_line,
        attributes,
        // The caller (`core.rs::parse_sections_from_str`) knows the whole
        // document's byte offsets and overwrites this with the real span
        // right after calling this function.
        span: crate::ast::SectionSpan::default(),
    })
}

/// Parse key=value options from section header inline options string.
/// Unknown keys and unparseable values are hard errors, not silently dropped
/// — a typo'd inline option (e.g. `tolerance=abc`) should fail the parse,
/// not run with a silently-defaulted value.
pub fn parse_inline_options(s: &str) -> Result<InlineOptions> {
    let mut inline_options = InlineOptions::default();

    for (key, value) in tokenize_inline_options(s) {
        match key.as_str() {
            "with_asserts" | "partial" | "unordered_arrays" => {
                let parsed = match value.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => anyhow::bail!("invalid boolean value for {key}: {value}"),
                };
                match key.as_str() {
                    "with_asserts" => inline_options.with_asserts = parsed,
                    "partial" => inline_options.partial = parsed,
                    _ => inline_options.unordered_arrays = parsed,
                }
            }
            "tolerance" => {
                // Digit separators (`1_000`) are valid in JSON5 payloads —
                // accept them here too instead of erroring on a form that
                // works everywhere else a number can appear.
                inline_options.tolerance =
                    Some(value.replace('_', "").parse::<f64>().map_err(|_| {
                        anyhow::anyhow!("invalid numeric value for tolerance: {value}")
                    })?);
            }
            "redact" => {
                let redact_str = value.trim().trim_matches('[').trim_matches(']');
                let strings: Vec<String> = redact_str
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                inline_options.redact = strings;
            }
            _ if is_extra_inline_option_key(&key) => {
                inline_options.extra.insert(key, value);
            }
            _ => anyhow::bail!("unknown inline option: {key}"),
        }
    }

    Ok(inline_options)
}

/// Parse a GCTF attribute from `#[name(value)]` content string.
/// Returns `None` if content is empty or invalid.
pub fn parse_attribute(content: &str) -> Option<GctfAttribute> {
    let content = content.trim();
    if content.is_empty() {
        return None;
    }

    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len && is_attr_name_char(bytes[pos]) {
        pos += 1;
    }

    if pos == 0 {
        return None;
    }

    let name = content[..pos].to_string();

    while pos < len && is_ws(bytes[pos]) {
        pos += 1;
    }

    if pos == len {
        return Some(GctfAttribute::flag(&name));
    }

    if bytes[pos] != b'(' {
        return None;
    }

    pos += 1;

    let value_start = pos;
    let mut paren_depth = 1;
    let mut escaped = false;

    while pos < len && paren_depth > 0 {
        if escaped {
            escaped = false;
            pos += 1;
            continue;
        }
        match bytes[pos] {
            b'\\' => {
                escaped = true;
                pos += 1;
            }
            b'"' | b'\'' => {
                let quote = bytes[pos];
                pos += 1;
                while pos < len {
                    if escaped {
                        escaped = false;
                        pos += 1;
                        continue;
                    }
                    if bytes[pos] == b'\\' {
                        escaped = true;
                        pos += 1;
                        continue;
                    }
                    if bytes[pos] == quote {
                        pos += 1;
                        break;
                    }
                    pos += 1;
                }
            }
            b'(' => {
                paren_depth += 1;
                pos += 1;
            }
            b')' => {
                paren_depth -= 1;
                pos += 1;
            }
            _ => pos += 1,
        }
    }

    if paren_depth != 0 {
        return None;
    }

    let value = content[value_start..pos - 1].to_string();
    Some(GctfAttribute::new(&name, &value))
}

fn is_attr_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Resolve attributes for a section, applying inheritance rules:
/// - Attributes from parent sections apply to child sections
/// - Child section attributes override parent attributes
/// - Attributes with the same name are overridden (not merged)
pub fn resolve_attributes(
    section_attrs: &[GctfAttribute],
    inherited_attrs: &[GctfAttribute],
) -> Vec<GctfAttribute> {
    let mut resolved: Vec<GctfAttribute> = inherited_attrs.to_vec();
    let mut seen: std::collections::HashSet<String> =
        inherited_attrs.iter().map(|a| a.name.clone()).collect();

    for attr in section_attrs {
        if seen.contains(&attr.name) {
            let idx = resolved.iter().position(|a| a.name == attr.name).unwrap();
            resolved[idx] = attr.clone();
        } else {
            resolved.push(attr.clone());
            seen.insert(attr.name.clone());
        }
    }

    resolved
}

/// Attributes that configure one section only. `#[skip]` is documented as
/// "the test continues with subsequent sections" and `#[repeat(N)]` as
/// "re-execute *this* section", so neither may propagate — inheriting them
/// let a single `#[skip]` disable the whole rest of the test while the run
/// still reported a pass.
pub const SECTION_SCOPED_ATTRIBUTES: &[&str] = &["skip", "repeat"];

/// The subset of a section's resolved attributes that later sections inherit.
pub fn inheritable_attributes(resolved: &[GctfAttribute]) -> Vec<GctfAttribute> {
    resolved
        .iter()
        .filter(|a| !SECTION_SCOPED_ATTRIBUTES.contains(&a.name.as_str()))
        .cloned()
        .collect()
}

/// Parse key-value section (one per line: key: value).
fn parse_key_value_section(content: &str) -> Result<crate::ast::OrderedStringMap> {
    let mut key_values = crate::ast::OrderedStringMap::new();

    for line in content.lines() {
        if let Some((key, value)) = tokenize_kv_line(line) {
            if key_values.contains_key(&key) {
                anyhow::bail!(
                    "duplicate key '{key}' — each key may only be set once per section (the second occurrence would silently win)"
                );
            }
            key_values.insert(key, value);
        }
    }

    Ok(key_values)
}

/// Like `parse_key_value_section`, but an indented line is appended (with its
/// original indentation) to the previous key's value instead of being
/// tokenized on its own — needed for `sources:`'s nested YAML list.
/// BENCH is the one key-value section whose values may span lines: `sources:`
/// carries a nested YAML list on indented continuation lines. Shared with the
/// recovering parser so both paths agree on what `sources` contains.
pub(crate) fn parse_bench_section(content: &str) -> Result<crate::ast::OrderedStringMap> {
    let mut key_values: crate::ast::OrderedStringMap = crate::ast::OrderedStringMap::new();
    let mut current_key: Option<String> = None;

    for line in content.lines() {
        let is_continuation = line.starts_with(' ') || line.starts_with('\t');
        if is_continuation
            && let Some(key) = &current_key
            && let Some(value) = key_values.get_mut(key)
        {
            value.push('\n');
            value.push_str(line);
            continue;
        }

        if let Some((key, value)) = tokenize_kv_line(line) {
            key_values.insert(key.clone(), value);
            current_key = Some(key);
        }
    }

    Ok(key_values)
}

/// Parse assertions section (one assertion per line).
fn parse_assertions(content: &str) -> Result<Vec<String>> {
    // No normalization needed — regex literals /pattern/ are now handled
    // by the assertion AST parser as Expr::RegExp nodes.
    let assertions: Vec<String> = content
        .lines()
        .filter_map(strip_assertion_comments)
        .collect();

    Ok(assertions)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: a comment line above the address became the address. The
    // file still dialed, `check` still passed, and `explain` reported
    // `Address: // staging only`.
    #[test]
    fn a_comment_line_is_not_part_of_a_single_value_section() {
        for (input, expected) in [
            ("// staging only\nlocalhost:4770", "localhost:4770"),
            ("# staging only\nlocalhost:4770", "localhost:4770"),
            ("localhost:4770\n// trailing note", "localhost:4770"),
            ("localhost:4770", "localhost:4770"),
        ] {
            assert_eq!(
                parse_section_content(SectionType::Address, input).unwrap(),
                SectionContent::Single(expected.to_string()),
                "input: {input:?}"
            );
        }

        assert_eq!(
            parse_section_content(SectionType::Endpoint, "// note\ns.S/M").unwrap(),
            SectionContent::Single("s.S/M".to_string())
        );
        // Comments only: nothing is left to dial.
        assert_eq!(
            parse_section_content(SectionType::Address, "// note").unwrap(),
            SectionContent::Empty
        );
    }

    #[test]
    fn parse_bench_section_keeps_nested_sources_as_one_value() {
        let content = "mode: fixed\nsources:\n  - name: users\n    file: data/users.csv\n  - name: orders\n    file: data/orders.csv\n";
        let kv = parse_bench_section(content).unwrap();
        assert_eq!(kv.get("mode").map(String::as_str), Some("fixed"));
        let sources = kv.get("sources").expect("sources key");
        // No stray top-level keys from the nested list.
        assert!(!kv.contains_key("- name"));
        assert!(!kv.contains_key("file"));
        let defs: Vec<serde_yaml_ng::Value> = serde_yaml_ng::from_str(sources).unwrap();
        assert_eq!(defs.len(), 2);
    }

    #[test]
    fn key_values_and_extract_preserve_source_order_not_hash_order() {
        // §2.3: KV/EXTRACT storage is insertion-ordered (IndexMap), so iteration
        // follows the author's source order deterministically — a HashMap would
        // give a per-process-random order and make this assertion flaky.
        let opts = "zeta: 1\nalpha: 2\nmiddle: 3\nbeta: 4\n";
        let SectionContent::KeyValues(kv) =
            parse_section_content(SectionType::Options, opts).unwrap()
        else {
            panic!("expected KeyValues");
        };
        assert_eq!(
            kv.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["zeta", "alpha", "middle", "beta"]
        );

        let extract = "zulu = .a\nmike = .b\nalpha = .c\n";
        let SectionContent::Extract(vars) =
            parse_section_content(SectionType::Extract, extract).unwrap()
        else {
            panic!("expected Extract");
        };
        assert_eq!(
            vars.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["zulu", "mike", "alpha"]
        );
    }

    #[test]
    fn parse_bench_section_flat_keys_unaffected() {
        let content = "mode: fixed\nduration: 30s\nconcurrency: 16\n";
        let kv = parse_bench_section(content).unwrap();
        assert_eq!(kv.get("mode").map(String::as_str), Some("fixed"));
        assert_eq!(kv.get("duration").map(String::as_str), Some("30s"));
        assert_eq!(kv.get("concurrency").map(String::as_str), Some("16"));
    }

    #[test]
    fn parse_dataset_section_valid_rows() {
        let content = "- id: \"1\"\n  name: Ada\n- id: \"2\"\n  name: Grace\n";
        let result = parse_section_content(SectionType::Dataset, content).unwrap();
        let SectionContent::Rows(rows) = result else {
            panic!("expected Rows content");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], serde_json::json!("1"));
        assert_eq!(rows[1]["name"], serde_json::json!("Grace"));
    }

    #[test]
    fn parse_dataset_section_rejects_non_object_row() {
        let content = "- id: \"1\"\n- 42\n";
        let err = parse_section_content(SectionType::Dataset, content).unwrap_err();
        assert!(err.to_string().contains("row 1"), "{err}");
    }

    #[test]
    fn parse_dataset_section_rejects_malformed_yaml() {
        let content = "not: [a, list, of, objects";
        assert!(parse_section_content(SectionType::Dataset, content).is_err());
    }

    #[test]
    fn parse_dataset_section_preserves_nested_structure() {
        // Unlike `--data` (CSV/TSV, everything a flat string), DATASET's
        // native YAML keeps real types and nested objects.
        let content = "- id: 1\n  active: true\n  meta:\n    tier: gold\n";
        let result = parse_section_content(SectionType::Dataset, content).unwrap();
        let SectionContent::Rows(rows) = result else {
            panic!("expected Rows content");
        };
        assert_eq!(rows[0]["id"], serde_json::json!(1));
        assert_eq!(rows[0]["active"], serde_json::json!(true));
        assert_eq!(rows[0]["meta"]["tier"], serde_json::json!("gold"));
    }

    #[test]
    fn tokenize_options() {
        let result = tokenize_inline_options("key1=value1 key2=value2");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("key1".into(), "value1".into()));
        assert_eq!(result[1], ("key2".into(), "value2".into()));
    }

    #[test]
    fn test_parse_inline_options() {
        let result = parse_inline_options("with_asserts=true partial=false tolerance=0.1").unwrap();
        assert!(result.with_asserts);
        assert!(!result.partial);
        assert_eq!(result.tolerance, Some(0.1));
    }

    #[test]
    fn parse_section_content_single_value() {
        let result = parse_section_content(SectionType::Address, "localhost:50051").unwrap();
        assert_eq!(
            result,
            SectionContent::Single("localhost:50051".to_string())
        );
    }

    #[test]
    fn parse_section_content_empty() {
        let result = parse_section_content(SectionType::Address, "").unwrap();
        assert_eq!(result, SectionContent::Empty);
    }

    #[test]
    fn parse_section_content_whitespace() {
        let result = parse_section_content(SectionType::Address, "   ").unwrap();
        assert_eq!(result, SectionContent::Empty);
    }

    #[test]
    fn parse_section_content_endpoint() {
        let result = parse_section_content(SectionType::Endpoint, "pkg.Service/Method").unwrap();
        assert_eq!(
            result,
            SectionContent::Single("pkg.Service/Method".to_string())
        );
    }

    #[test]
    fn parse_section_content_request_json() {
        let result = parse_section_content(SectionType::Request, r#"{"key": "value"}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn parse_section_content_error_json() {
        let result = parse_section_content(SectionType::Error, r#"{"code": 5}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn parse_section_content_response_json() {
        let result = parse_section_content(SectionType::Response, r#"{"status": "ok"}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn parse_section_content_response_jsonlines() {
        let input = "{\"a\":1}\n{\"b\":2}";
        let result = parse_section_content(SectionType::Response, input).unwrap();
        assert!(matches!(result, SectionContent::JsonLines(v) if v.len() == 2));
    }

    #[test]
    fn parse_section_content_request_jsonlines() {
        // Symmetric with RESPONSE: a REQUEST block of self-delimiting JSON
        // values (client/bidi-streaming) parses into JsonLines, same as a
        // multi-message RESPONSE.
        let input = "{\"a\":1}\n{\"b\":2}\n{\"c\":3}";
        let result = parse_section_content(SectionType::Request, input).unwrap();
        assert!(matches!(result, SectionContent::JsonLines(v) if v.len() == 3));
    }

    #[test]
    fn parse_section_content_request_single_value_stays_json() {
        // A single-value REQUEST must stay unary — existing files unchanged.
        let result = parse_section_content(SectionType::Request, r#"{"key": "value"}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn parse_section_content_key_values() {
        let input = "ca_cert: /path/to/ca.pem\nserver_name: example.com";
        let result = parse_section_content(SectionType::Tls, input).unwrap();
        if let SectionContent::KeyValues(kv) = result {
            assert_eq!(kv.get("ca_cert"), Some(&"/path/to/ca.pem".to_string()));
            assert_eq!(kv.get("server_name"), Some(&"example.com".to_string()));
        } else {
            panic!("expected KeyValues");
        }
    }

    #[test]
    fn parse_section_content_key_values_with_comments() {
        let input = "# comment\nca_cert: /path/ca.pem\n\nkey: value";
        let result = parse_section_content(SectionType::Options, input).unwrap();
        if let SectionContent::KeyValues(kv) = result {
            assert_eq!(kv.len(), 2);
        } else {
            panic!("expected KeyValues");
        }
    }

    #[test]
    fn parse_section_content_extract() {
        let input = "total = .response.total\ncount = .items | length";
        let result = parse_section_content(SectionType::Extract, input).unwrap();
        if let SectionContent::Extract(kv) = result {
            assert_eq!(kv.get("total"), Some(&".response.total".to_string()));
            assert!(kv.contains_key("count"));
        } else {
            panic!("expected Extract");
        }
    }

    #[test]
    fn parse_section_content_extract_with_comments() {
        let input = "# ignore\n// ignore\ntotal = .response.total";
        let result = parse_section_content(SectionType::Extract, input).unwrap();
        if let SectionContent::Extract(kv) = result {
            assert_eq!(kv.len(), 1);
        } else {
            panic!("expected Extract");
        }
    }

    #[test]
    fn parse_section_content_key_values_duplicate_key_is_an_error() {
        let input = "timeout: 30\ntimeout: 60";
        let err = parse_section_content(SectionType::Options, input).unwrap_err();
        assert!(err.to_string().contains("duplicate key 'timeout'"), "{err}");
    }

    #[test]
    fn parse_section_content_extract_duplicate_variable_is_an_error() {
        let input = "total = .a\ntotal = .b";
        let err = parse_section_content(SectionType::Extract, input).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate EXTRACT variable 'total'"),
            "{err}"
        );
    }

    #[test]
    fn parse_section_content_asserts() {
        let input = ".x == 1\n.y != \"hello\"";
        let result = parse_section_content(SectionType::Asserts, input).unwrap();
        if let SectionContent::Assertions(asserts) = result {
            assert_eq!(asserts.len(), 2);
            assert_eq!(asserts[0], ".x == 1");
        } else {
            panic!("expected Assertions");
        }
    }

    #[test]
    fn parse_section_content_asserts_with_comments() {
        let input = ".x == 1 # inline\n# full line\n.y == 2 // comment";
        let result = parse_section_content(SectionType::Asserts, input).unwrap();
        if let SectionContent::Assertions(asserts) = result {
            assert_eq!(asserts.len(), 2);
        } else {
            panic!("expected Assertions");
        }
    }

    #[test]
    fn test_build_section() {
        let content = vec!["localhost:50051".to_string()];
        let section = build_section(
            SectionType::Address,
            5,
            6,
            &content,
            InlineOptions::default(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(section.section_type, SectionType::Address);
        assert_eq!(section.start_line, 5);
        assert_eq!(section.end_line, 6);
    }

    #[test]
    fn parse_inline_options_all_fields() {
        let result = parse_inline_options(
            "with_asserts=true partial=true tolerance=0.5 unordered_arrays=true",
        )
        .unwrap();
        assert!(result.with_asserts);
        assert!(result.partial);
        assert_eq!(result.tolerance, Some(0.5));
        assert!(result.unordered_arrays);
    }

    #[test]
    fn parse_inline_options_redact() {
        let result = parse_inline_options(r#"redact=["token","password"]"#).unwrap();
        assert_eq!(result.redact, vec!["token", "password"]);
    }

    #[test]
    fn parse_inline_options_empty() {
        let result = parse_inline_options("").unwrap();
        assert_eq!(result, InlineOptions::default());
    }

    #[test]
    fn parse_inline_options_unknown_key_errors() {
        let err = parse_inline_options("unknown_key=value").unwrap_err();
        assert!(err.to_string().contains("unknown_key"));
    }

    #[test]
    fn parse_inline_options_invalid_boolean_errors() {
        // §3.4 fixed this from a silent fallback to a hard error in the strict
        // path — a non-boolean value for a boolean option must not be silently
        // accepted.
        let err = parse_inline_options("with_asserts=maybe").unwrap_err();
        assert!(err.to_string().contains("with_asserts"), "{err}");
    }

    #[test]
    fn parse_section_content_meta_malformed_yaml_errors() {
        // §3.1: malformed META YAML must hard-error in the strict path, not
        // silently default to an empty FileMeta.
        let err = parse_section_content(SectionType::Meta, "name: [unterminated").unwrap_err();
        assert!(err.to_string().contains("META"), "{err}");
    }

    #[test]
    fn parse_section_content_meta_unknown_field_errors() {
        // §3.2: `deny_unknown_fields` — a `tag:` typo (real field is `tags:`)
        // must error, not silently vanish.
        let err = parse_section_content(SectionType::Meta, "tag: oops").unwrap_err();
        assert!(err.to_string().contains("META"), "{err}");
    }

    #[test]
    fn parse_inline_options_tolerance_negative() {
        let result = parse_inline_options("tolerance=-0.5").unwrap();
        assert_eq!(result.tolerance, Some(-0.5));
    }

    #[test]
    fn parse_inline_options_tolerance_digit_separator() {
        let result = parse_inline_options("tolerance=1_000.5").unwrap();
        assert_eq!(result.tolerance, Some(1000.5));
    }

    #[test]
    fn parse_inline_options_tolerance_invalid() {
        let err = parse_inline_options("tolerance=not_a_number").unwrap_err();
        assert!(err.to_string().contains("tolerance"));
    }

    #[test]
    fn parse_inline_options_redact_with_spaces() {
        let result = parse_inline_options(r#"redact=["token", "password"]"#).unwrap();
        assert_eq!(result.redact, vec!["token", "password"]);
    }

    #[test]
    fn parse_inline_options_redact_empty_array() {
        let result = parse_inline_options("redact=[]").unwrap();
        assert!(result.redact.is_empty());
    }

    #[test]
    fn parse_inline_options_redact_malformed() {
        let result = parse_inline_options("redact=not_an_array").unwrap();
        // Current tokenizer splits by spaces, so this becomes tokens
        // This is a known limitation - redact with spaces in value
        assert!(!result.redact.is_empty()); // tokenizer splits "not_an_array" into parts
    }

    #[test]
    fn parse_section_content_meta_full() {
        let result = parse_section_content(
            SectionType::Meta,
            r#"name: Test
summary: Summary
tags: [a, b]
owner: backend
links:
  - https://example.com
"#,
        )
        .unwrap();
        let SectionContent::Meta(m) = result else {
            panic!()
        };
        assert_eq!(m.name.as_deref(), Some("Test"));
        assert_eq!(m.summary.as_deref(), Some("Summary"));
        assert_eq!(m.tags, ["a", "b"]);
        assert_eq!(m.owner.as_deref(), Some("backend"));
        assert_eq!(m.links, ["https://example.com"]);
    }

    #[test]
    fn parse_section_content_meta_comments() {
        let result = parse_section_content(
            SectionType::Meta,
            r#"# comment
name: Test
tags: [a]
"#,
        )
        .unwrap();
        let SectionContent::Meta(m) = result else {
            panic!()
        };
        assert_eq!(m.name.as_deref(), Some("Test"));
        assert_eq!(m.tags, ["a"]);
    }

    #[test]
    fn parse_section_content_meta_slash_comments() {
        // Regression: the strict path used to pass META content straight to
        // the YAML parser, which errors on GCTF's `//` comment (YAML only
        // understands `#`) — so a `//`-commented META parsed fine under the
        // lenient path (`run`) but hard-failed under `check`/`fmt`. Now both
        // strip `//` first via the shared `strip_gctf_comment_lines`.
        let result = parse_section_content(
            SectionType::Meta,
            "// a GCTF comment\nname: Test\ntags: [a]\n",
        )
        .unwrap();
        let SectionContent::Meta(m) = result else {
            panic!()
        };
        assert_eq!(m.name.as_deref(), Some("Test"));
        assert_eq!(m.tags, ["a"]);
    }

    #[test]
    fn parse_dataset_section_slash_comments() {
        let result = parse_section_content(
            SectionType::Dataset,
            "// header comment\n- id: \"1\"\n  name: Ada\n",
        )
        .unwrap();
        let SectionContent::Rows(rows) = result else {
            panic!("expected Rows content");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], serde_json::json!("Ada"));
    }

    #[test]
    fn parse_attribute_with_value() {
        let attr = parse_attribute("timeout(30)").unwrap();
        assert_eq!(attr.name, "timeout");
        assert_eq!(attr.value, "30");
        assert_eq!(attr.parse_u64(), Some(30));
    }

    #[test]
    fn parse_attribute_flag() {
        let attr = parse_attribute("skip").unwrap();
        assert_eq!(attr.name, "skip");
        assert_eq!(attr.value, "true");
        assert_eq!(attr.parse_bool(), Some(true));
    }

    #[test]
    fn parse_attribute_quoted_value() {
        let attr = parse_attribute(r#"tag("smoke, slow")"#).unwrap();
        assert_eq!(attr.name, "tag");
        assert_eq!(attr.value, r#""smoke, slow""#);
    }

    #[test]
    fn parse_attribute_with_spaces() {
        let attr = parse_attribute("  retry(3)  ").unwrap();
        assert_eq!(attr.name, "retry");
        assert_eq!(attr.value, "3");
    }

    #[test]
    fn parse_attribute_empty() {
        assert!(parse_attribute("").is_none());
        assert!(parse_attribute("   ").is_none());
    }

    #[test]
    fn parse_attribute_no_paren() {
        let attr = parse_attribute("just_a_name").unwrap();
        assert_eq!(attr.name, "just_a_name");
        assert_eq!(attr.value, "true");
    }

    #[test]
    fn resolve_attributes_inheritance() {
        let parent = vec![GctfAttribute::new("timeout", "10")];
        let child = vec![GctfAttribute::new("retry", "3")];
        let resolved = resolve_attributes(&child, &parent);

        let timeout = resolved.iter().find(|a| a.name == "timeout");
        let retry = resolved.iter().find(|a| a.name == "retry");

        assert_eq!(timeout.map(|a| a.value.as_str()), Some("10"));
        assert_eq!(retry.map(|a| a.value.as_str()), Some("3"));
    }

    #[test]
    fn resolve_attributes_override() {
        let parent = vec![GctfAttribute::new("timeout", "10")];
        let child = vec![GctfAttribute::new("timeout", "30")];
        let resolved = resolve_attributes(&child, &parent);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].value, "30");
    }

    #[test]
    fn resolve_attributes_empty() {
        let resolved = resolve_attributes(&[], &[]);
        assert!(resolved.is_empty());

        let parent = vec![GctfAttribute::new("timeout", "10")];
        let resolved = resolve_attributes(&[], &parent);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].value, "10");
    }
}
