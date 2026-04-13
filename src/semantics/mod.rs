use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::parser;
use crate::parser::tokenizer::{TokenKind, tokenize_assertion};
use crate::plugins::{PluginManager, PluginSignature, TypeInfo};
use crate::utils::section_content_line;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionTypeMismatch {
    pub rule_id: String,
    pub line: usize,
    pub expression: String,
    pub message: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownPluginCall {
    pub rule_id: String,
    pub line: usize,
    pub expression: String,
    pub plugin_name: String,
    pub message: String,
    pub suggestion: Option<String>,
}

fn operator_from_tokens(
    tokens: &[parser::tokenizer::Token],
) -> Option<(&'static str, usize, usize)> {
    for token in tokens {
        if let TokenKind::Op(op) = &token.kind {
            let static_op: Option<&'static str> = match op.as_str() {
                "==" => Some("=="),
                "!=" => Some("!="),
                ">=" => Some(">="),
                "<=" => Some("<="),
                ">" => Some(">"),
                "<" => Some("<"),
                "contains" => Some("contains"),
                "matches" => Some("matches"),
                "startsWith" => Some("startsWith"),
                "endsWith" => Some("endsWith"),
                _ => None,
            };
            if let Some(s) = static_op {
                return Some((s, token.span.start, token.span.len()));
            }
        }
    }
    None
}

fn plugin_signatures() -> &'static HashMap<String, PluginSignature> {
    use crate::plugins::PLUGIN_SIGNATURES;
    &PLUGIN_SIGNATURES
}

fn extract_plugin_calls(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut calls = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }

        let start = i + 1;
        let mut end = start;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }

        if end == start {
            i += 1;
            continue;
        }

        let mut cursor = end;
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }

        if cursor < chars.len() && chars[cursor] == '(' {
            let name: String = chars[start..end].iter().collect();
            calls.push(name);
        }

        i = end;
    }

    calls
}

fn best_plugin_suggestion(unknown: &str, known_plugins: &[String]) -> Option<String> {
    fn common_prefix_len(a: &str, b: &str) -> usize {
        a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
    }

    let mut best: Option<(&str, usize, usize)> = None;
    for candidate in known_plugins {
        let prefix = common_prefix_len(unknown, candidate);
        let len_diff = unknown.len().abs_diff(candidate.len());

        match best {
            None => best = Some((candidate.as_str(), prefix, len_diff)),
            Some((_, best_prefix, best_len_diff)) => {
                if prefix > best_prefix || (prefix == best_prefix && len_diff < best_len_diff) {
                    best = Some((candidate.as_str(), prefix, len_diff));
                }
            }
        }
    }

    best.and_then(|(name, prefix, _)| {
        if prefix >= 3 {
            Some(name.to_string())
        } else {
            None
        }
    })
}

fn infer_type_from_tokens(
    tokens: &[parser::tokenizer::Token],
    signatures: &HashMap<String, PluginSignature>,
) -> TypeInfo {
    if tokens.len() == 1 {
        return match &tokens[0].kind {
            TokenKind::StringLit(_) => TypeInfo::String,
            TokenKind::NumberLit(v) if v.parse::<f64>().is_ok() => TypeInfo::Number,
            TokenKind::Ident(s) if s == "true" || s == "false" => TypeInfo::Bool,
            TokenKind::LBracket => TypeInfo::Any,
            TokenKind::LBrace => TypeInfo::Any,
            _ => TypeInfo::Any,
        };
    }

    if tokens.len() >= 3
        && matches!(&tokens[0].kind, TokenKind::At)
        && matches!(&tokens[1].kind, TokenKind::Ident(name) if {
            if let Some(sig) = signatures.get(name.as_str()) {
                return sig.return_type;
            }
            false
        })
    {
        return TypeInfo::Any;
    }

    for token in tokens {
        if let TokenKind::StringLit(_) = &token.kind {
            return TypeInfo::String;
        }
    }

    TypeInfo::Any
}

fn detect_type_mismatch(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<AssertionTypeMismatch> {
    let tokens = tokenize_assertion(expr);
    let (op, op_idx, op_len) = operator_from_tokens(&tokens)?;
    let lhs = expr[..op_idx].trim();
    let rhs = expr[op_idx + op_len..].trim();
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }

    let lhs_tokens = tokenize_assertion(lhs);
    let rhs_tokens = tokenize_assertion(rhs);
    let lhs_type = infer_type_from_tokens(&lhs_tokens, signatures);
    let rhs_type = infer_type_from_tokens(&rhs_tokens, signatures);

    // Check if the operator is valid for the left-hand side type
    let (valid, reason) = lhs_type.supports_operator(op);
    if !valid {
        return Some(AssertionTypeMismatch {
            rule_id: "SEM_T005".to_string(),
            line: 0,
            expression: expr.to_string(),
            message: format!(
                "Operator '{}' is not valid for {}: {}",
                op,
                lhs_type.display_name(),
                reason.unwrap_or("")
            ),
            expected: format!("a type that supports '{}'", op),
            actual: lhs_type.display_name().to_string(),
        });
    }

    // For comparison operators, also check type compatibility between LHS and RHS
    if op == "==" || op == "!=" {
        // Equality is allowed between most types, but flag obvious mismatches
        if lhs_type != TypeInfo::Any
            && rhs_type != TypeInfo::Any
            && !types_compatible(lhs_type, rhs_type)
        {
            return Some(AssertionTypeMismatch {
                rule_id: "SEM_T001".to_string(),
                line: 0,
                expression: expr.to_string(),
                message: format!(
                    "Type-incompatible comparison: {} is {}, but {} is {}",
                    lhs,
                    lhs_type.display_name(),
                    rhs,
                    rhs_type.display_name()
                ),
                expected: lhs_type.display_name().to_string(),
                actual: rhs_type.display_name().to_string(),
            });
        }
    }

    if matches!(op, ">" | "<" | ">=" | "<=") && !rhs_type.is_numeric() && rhs_type != TypeInfo::Any
    {
        return Some(AssertionTypeMismatch {
            rule_id: "SEM_T002".to_string(),
            line: 0,
            expression: expr.to_string(),
            message: format!(
                "Ordering operator '{}' requires a number on the right, but {} is {}",
                op,
                rhs,
                rhs_type.display_name()
            ),
            expected: "number".to_string(),
            actual: rhs_type.display_name().to_string(),
        });
    }

    if matches!(op, "contains" | "startsWith" | "endsWith" | "matches")
        && !rhs_type.is_stringy()
        && rhs_type != TypeInfo::Any
    {
        return Some(AssertionTypeMismatch {
            rule_id: "SEM_T003".to_string(),
            line: 0,
            expression: expr.to_string(),
            message: format!(
                "Operator '{}' requires a string on the right, but {} is {}",
                op,
                rhs,
                rhs_type.display_name()
            ),
            expected: "string".to_string(),
            actual: rhs_type.display_name().to_string(),
        });
    }

    None
}

/// Check if two types can be reasonably compared with ==/!=.
fn types_compatible(a: TypeInfo, b: TypeInfo) -> bool {
    if a == b {
        return true;
    }
    // Numeric types are compatible
    if a.is_numeric() && b.is_numeric() {
        return true;
    }
    // String-like types are compatible
    if a.is_stringy() && b.is_stringy() {
        return true;
    }
    // Bool is compatible with BoolOrNull
    if matches!(a, TypeInfo::Bool | TypeInfo::BoolOrNull)
        && matches!(b, TypeInfo::Bool | TypeInfo::BoolOrNull)
    {
        return true;
    }
    // Unknown (Any) is compatible with anything
    if a == TypeInfo::Any || b == TypeInfo::Any {
        return true;
    }
    false
}

pub fn validate_plugin_semantics_completeness() -> Vec<String> {
    let mut issues = Vec::new();
    for plugin in PluginManager::new().list() {
        let name = plugin.name().to_string();
        let sig = plugin.signature();

        if sig.return_type == TypeInfo::Any {
            issues.push(format!("{}: return_type is Any (unknown)", name));
        }
        let _ = sig.arg_names;
    }
    issues
}

pub fn collect_assertion_type_mismatches(doc: &parser::GctfDocument) -> Vec<AssertionTypeMismatch> {
    let signatures = plugin_signatures();
    let mut mismatches = Vec::new();

    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };

            if let Some(mut mismatch) = detect_type_mismatch(&trimmed, signatures) {
                mismatch.line = section_content_line(section.start_line, idx);
                mismatches.push(mismatch);
            }
        }
    }

    mismatches
}

pub fn collect_unknown_plugin_calls(doc: &parser::GctfDocument) -> Vec<UnknownPluginCall> {
    let signatures = plugin_signatures();
    let mut known_plugins: Vec<String> = signatures.keys().cloned().collect();
    known_plugins.sort();

    let mut unknown = Vec::new();

    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };

            for plugin_name in extract_plugin_calls(&trimmed) {
                if signatures.contains_key(plugin_name.as_str()) {
                    continue;
                }

                let suggestion =
                    best_plugin_suggestion(&plugin_name, &known_plugins).map(|s| format!("@{}", s));
                let message = match &suggestion {
                    Some(s) => format!(
                        "Unknown assertion plugin '@{}'. Did you mean {}?",
                        plugin_name, s
                    ),
                    None => format!("Unknown assertion plugin '@{}'", plugin_name),
                };

                unknown.push(UnknownPluginCall {
                    rule_id: "SEM_F001".to_string(),
                    line: section_content_line(section.start_line, idx),
                    expression: trimmed.to_string(),
                    plugin_name,
                    message,
                    suggestion,
                });
            }
        }
    }

    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_detects_boolean_vs_number() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.names) == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].rule_id, "SEM_T001");
    }

    #[test]
    fn test_semantics_allows_boolean_compare() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x-request-id") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(mismatches.is_empty());
    }

    #[test]
    fn test_semantics_detects_startswith_non_string() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.names) startsWith "a"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        // SEM_T005: startsWith is not valid for non-string LHS (UInt from @len)
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn test_plugin_semantics_completeness() {
        let issues = validate_plugin_semantics_completeness();
        assert!(issues.is_empty(), "Incomplete plugin semantics: {issues:?}");
    }

    #[test]
    fn test_semantics_detects_unknown_plugin_calls() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@regexp(.name, "^a") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let unknown = collect_unknown_plugin_calls(&doc);
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].rule_id, "SEM_F001");
        assert_eq!(unknown[0].plugin_name, "regexp");
        assert_eq!(unknown[0].suggestion.as_deref(), Some("@regex"));
    }

    #[test]
    fn test_semantics_allows_known_plugin_calls() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@regex(.name, "^a") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let unknown = collect_unknown_plugin_calls(&doc);
        assert!(unknown.is_empty());
    }
}
