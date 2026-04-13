//! Section content parser for GCTF files.
//!
//! Parses the content of different section types based on their structure.

use anyhow::Result;

use crate::parser::ast::{InlineOptions, Section, SectionContent, SectionType};
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
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }

                // Parse using ternary AST
                if let Some(extract_var) = crate::parser::ternary_ast::ExtractVar::parse(trimmed) {
                    // Store the JQ-converted value for backward compatibility
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

/// Parse key=value options from string.
pub fn parse_inline_options(s: &str) -> Result<InlineOptions> {
    let options = parse_key_value_options(s)?;

    let mut inline_options = InlineOptions::default();

    if let Some(with_asserts) = options.get("with_asserts") {
        inline_options.with_asserts = matches!(with_asserts.as_str(), "true" | "1");
    }

    if let Some(partial) = options.get("partial") {
        inline_options.partial = matches!(partial.as_str(), "true" | "1");
    }

    if let Some(tolerance) = options.get("tolerance")
        && let Ok(t) = tolerance.parse::<f64>()
    {
        inline_options.tolerance = Some(t);
    }

    if let Some(redact) = options.get("redact") {
        // Parse array format: ["field1","field2"]
        let redact_str = redact.trim().trim_matches('[').trim_matches(']');
        let strings: Vec<String> = redact_str
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        inline_options.redact = strings;
    }

    if let Some(unnamed_arrays) = options.get("unordered_arrays") {
        inline_options.unordered_arrays = matches!(unnamed_arrays.as_str(), "true" | "1");
    }

    Ok(inline_options)
}

/// Parse key-value options from string.
pub fn parse_key_value_options(s: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut options = std::collections::HashMap::new();
    let tokens = tokenize_options(s)?;

    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            options.insert(key, value);
        } else {
            // Short boolean form: token without "=" means "true"
            let key = token.trim().to_string();
            options.insert(key, "true".to_string());
        }
    }

    Ok(options)
}

/// Tokenize options string, respecting quotes.
fn tokenize_options(s: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in s.chars() {
        match (ch, in_quotes, escaped) {
            ('\\', _, false) => {
                escaped = true;
                current_token.push(ch);
            }
            (_, _, true) => {
                escaped = false;
                current_token.push(ch);
            }
            ('"', false, _) => {
                in_quotes = true;
                current_token.push(ch);
            }
            ('"', true, _) => {
                in_quotes = false;
                current_token.push(ch);
            }
            (' ', false, _) => {
                if !current_token.is_empty() {
                    tokens.push(current_token.clone());
                    current_token.clear();
                }
            }
            _ => {
                current_token.push(ch);
            }
        }
    }

    if !current_token.is_empty() {
        tokens.push(current_token);
    }

    Ok(tokens)
}

/// Parse key-value section (one per line: key: value).
fn parse_key_value_section(content: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut key_values = std::collections::HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse key: value
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
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
        let result = parse_key_value_options("key1=value1 key2=value2").unwrap();
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));
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
}
