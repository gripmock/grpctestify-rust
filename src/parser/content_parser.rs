//! Section content parser for GCTF files.
//!
//! Parses the content of different section types based on their structure.

use anyhow::Result;

use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionType};
use crate::parser::gctf_tokenizer::{
    tokenize_extract_line, tokenize_inline_options, tokenize_kv_line,
};
use crate::parser::json_mod;
use crate::parser::json_stream_parser;

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
        | SectionType::Options => {
            let key_values = parse_key_value_section(content)?;
            Ok(SectionContent::KeyValues(key_values))
        }

        // Extract section - support ternary expressions via AST
        SectionType::Extract => {
            let mut key_values = std::collections::HashMap::new();
            for line in content.lines() {
                if let Some((name, value)) = tokenize_extract_line(line)
                    && let Some(extract_var) =
                        crate::parser::ternary_ast::ExtractVar::parse_raw(&name, &value)
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
    }
}

/// Build a section from parsed content.
pub fn build_section(
    section_type: SectionType,
    start_line: usize,
    end_line: usize,
    content: &[String],
    inline_options: InlineOptions,
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
    use crate::parser::assertions::strip_assertion_comments;

    // No normalization needed — regex literals /pattern/ are now handled
    // by the assertion AST parser as Expr::RegExp nodes.
    let assertions: Vec<String> = content
        .lines()
        .filter_map(strip_assertion_comments)
        .map(|line| line.to_string())
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
}
