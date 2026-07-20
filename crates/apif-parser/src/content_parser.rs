//! Section content parser for GCTF files.
//!
//! Parses the content of different section types based on their structure.

use anyhow::Result;

use crate::assertions::strip_assertion_comments;
use crate::ast::{FileMeta, GctfAttribute, InlineOptions, Section, SectionContent, SectionType};
use crate::gctf_tokenizer::{tokenize_extract_line, tokenize_inline_options, tokenize_kv_line};
use crate::json_mod;
use crate::json_stream_parser;

/// Parse section content based on section type.
pub fn parse_section_content(section_type: SectionType, content: &str) -> Result<SectionContent> {
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
        | SectionType::Options
        | SectionType::Bench => {
            let key_values = parse_key_value_section(content)?;
            Ok(SectionContent::KeyValues(key_values))
        }

        // Extract section - support ternary expressions via AST
        SectionType::Extract => {
            let mut key_values = std::collections::HashMap::new();
            for line in content.lines() {
                if let Some((name, value)) = tokenize_extract_line(line)
                    && let Some(extract_var) =
                        crate::ternary_ast::ExtractVar::parse_raw(&name, &value)
                {
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

        // META section - parse as YAML (comments allowed)
        SectionType::Meta => {
            let meta = serde_yaml_ng::from_str::<FileMeta>(content)
                .unwrap_or_else(|_| FileMeta::default());
            Ok(SectionContent::Meta(meta))
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
    })
}

/// Parse key=value options from section header inline options string.
pub fn parse_inline_options(s: &str) -> Result<InlineOptions> {
    let mut inline_options = InlineOptions::default();

    for (key, value) in tokenize_inline_options(s) {
        match key.as_str() {
            "with_asserts" => {
                inline_options.with_asserts = matches!(value.as_str(), "true" | "1");
            }
            "partial" => {
                inline_options.partial = matches!(value.as_str(), "true" | "1");
            }
            "tolerance" => {
                if let Ok(t) = value.parse::<f64>() {
                    inline_options.tolerance = Some(t);
                }
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
            "unordered_arrays" => {
                inline_options.unordered_arrays = matches!(value.as_str(), "true" | "1");
            }
            _ => {}
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

/// Parse key-value section (one per line: key: value).
fn parse_key_value_section(content: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut key_values = std::collections::HashMap::new();

    for line in content.lines() {
        if let Some((key, value)) = tokenize_kv_line(line) {
            key_values.insert(key, value);
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

    #[test]
    fn test_tokenize_options() {
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
    fn test_parse_section_content_single_value() {
        let result = parse_section_content(SectionType::Address, "localhost:50051").unwrap();
        assert_eq!(
            result,
            SectionContent::Single("localhost:50051".to_string())
        );
    }

    #[test]
    fn test_parse_section_content_empty() {
        let result = parse_section_content(SectionType::Address, "").unwrap();
        assert_eq!(result, SectionContent::Empty);
    }

    #[test]
    fn test_parse_section_content_whitespace() {
        let result = parse_section_content(SectionType::Address, "   ").unwrap();
        assert_eq!(result, SectionContent::Empty);
    }

    #[test]
    fn test_parse_section_content_endpoint() {
        let result = parse_section_content(SectionType::Endpoint, "pkg.Service/Method").unwrap();
        assert_eq!(
            result,
            SectionContent::Single("pkg.Service/Method".to_string())
        );
    }

    #[test]
    fn test_parse_section_content_request_json() {
        let result = parse_section_content(SectionType::Request, r#"{"key": "value"}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn test_parse_section_content_error_json() {
        let result = parse_section_content(SectionType::Error, r#"{"code": 5}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn test_parse_section_content_response_json() {
        let result = parse_section_content(SectionType::Response, r#"{"status": "ok"}"#).unwrap();
        assert!(matches!(result, SectionContent::Json(_)));
    }

    #[test]
    fn test_parse_section_content_response_jsonlines() {
        let input = "{\"a\":1}\n{\"b\":2}";
        let result = parse_section_content(SectionType::Response, input).unwrap();
        assert!(matches!(result, SectionContent::JsonLines(v) if v.len() == 2));
    }

    #[test]
    fn test_parse_section_content_key_values() {
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
    fn test_parse_section_content_key_values_with_comments() {
        let input = "# comment\nca_cert: /path/ca.pem\n\nkey: value";
        let result = parse_section_content(SectionType::Options, input).unwrap();
        if let SectionContent::KeyValues(kv) = result {
            assert_eq!(kv.len(), 2);
        } else {
            panic!("expected KeyValues");
        }
    }

    #[test]
    fn test_parse_section_content_extract() {
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
    fn test_parse_section_content_extract_with_comments() {
        let input = "# ignore\n// ignore\ntotal = .response.total";
        let result = parse_section_content(SectionType::Extract, input).unwrap();
        if let SectionContent::Extract(kv) = result {
            assert_eq!(kv.len(), 1);
        } else {
            panic!("expected Extract");
        }
    }

    #[test]
    fn test_parse_section_content_asserts() {
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
    fn test_parse_section_content_asserts_with_comments() {
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
    fn test_parse_inline_options_all_fields() {
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
    fn test_parse_inline_options_redact() {
        let result = parse_inline_options(r#"redact=["token","password"]"#).unwrap();
        assert_eq!(result.redact, vec!["token", "password"]);
    }

    #[test]
    fn test_parse_inline_options_empty() {
        let result = parse_inline_options("").unwrap();
        assert_eq!(result, InlineOptions::default());
    }

    #[test]
    fn test_parse_inline_options_unknown_key_ignored() {
        let result = parse_inline_options("unknown_key=value").unwrap();
        assert_eq!(result, InlineOptions::default());
    }

    #[test]
    fn test_parse_inline_options_tolerance_negative() {
        let result = parse_inline_options("tolerance=-0.5").unwrap();
        assert_eq!(result.tolerance, Some(-0.5));
    }

    #[test]
    fn test_parse_inline_options_tolerance_invalid() {
        let result = parse_inline_options("tolerance=not_a_number").unwrap();
        assert_eq!(result.tolerance, None);
    }

    #[test]
    fn test_parse_inline_options_redact_with_spaces() {
        let result = parse_inline_options(r#"redact=["token", "password"]"#).unwrap();
        assert_eq!(result.redact, vec!["token", "password"]);
    }

    #[test]
    fn test_parse_inline_options_redact_empty_array() {
        let result = parse_inline_options("redact=[]").unwrap();
        assert!(result.redact.is_empty());
    }

    #[test]
    fn test_parse_inline_options_redact_malformed() {
        let result = parse_inline_options("redact=not_an_array").unwrap();
        // Current tokenizer splits by spaces, so this becomes tokens
        // This is a known limitation - redact with spaces in value
        assert!(!result.redact.is_empty()); // tokenizer splits "not_an_array" into parts
    }

    #[test]
    fn test_parse_section_content_meta_full() {
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
    fn test_parse_section_content_meta_comments() {
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
    fn test_parse_attribute_with_value() {
        let attr = parse_attribute("timeout(30)").unwrap();
        assert_eq!(attr.name, "timeout");
        assert_eq!(attr.value, "30");
        assert_eq!(attr.parse_u64(), Some(30));
    }

    #[test]
    fn test_parse_attribute_flag() {
        let attr = parse_attribute("skip").unwrap();
        assert_eq!(attr.name, "skip");
        assert_eq!(attr.value, "true");
        assert_eq!(attr.parse_bool(), Some(true));
    }

    #[test]
    fn test_parse_attribute_quoted_value() {
        let attr = parse_attribute(r#"tag("smoke, slow")"#).unwrap();
        assert_eq!(attr.name, "tag");
        assert_eq!(attr.value, r#""smoke, slow""#);
    }

    #[test]
    fn test_parse_attribute_with_spaces() {
        let attr = parse_attribute("  retry(3)  ").unwrap();
        assert_eq!(attr.name, "retry");
        assert_eq!(attr.value, "3");
    }

    #[test]
    fn test_parse_attribute_empty() {
        assert!(parse_attribute("").is_none());
        assert!(parse_attribute("   ").is_none());
    }

    #[test]
    fn test_parse_attribute_no_paren() {
        let attr = parse_attribute("just_a_name").unwrap();
        assert_eq!(attr.name, "just_a_name");
        assert_eq!(attr.value, "true");
    }

    #[test]
    fn test_resolve_attributes_inheritance() {
        let parent = vec![GctfAttribute::new("timeout", "10")];
        let child = vec![GctfAttribute::new("retry", "3")];
        let resolved = resolve_attributes(&child, &parent);

        let timeout = resolved.iter().find(|a| a.name == "timeout");
        let retry = resolved.iter().find(|a| a.name == "retry");

        assert_eq!(timeout.map(|a| a.value.as_str()), Some("10"));
        assert_eq!(retry.map(|a| a.value.as_str()), Some("3"));
    }

    #[test]
    fn test_resolve_attributes_override() {
        let parent = vec![GctfAttribute::new("timeout", "10")];
        let child = vec![GctfAttribute::new("timeout", "30")];
        let resolved = resolve_attributes(&child, &parent);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].value, "30");
    }

    #[test]
    fn test_resolve_attributes_empty() {
        let resolved = resolve_attributes(&[], &[]);
        assert!(resolved.is_empty());

        let parent = vec![GctfAttribute::new("timeout", "10")];
        let resolved = resolve_attributes(&[], &parent);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].value, "10");
    }
}
