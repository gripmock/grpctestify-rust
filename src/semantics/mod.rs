use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::parser;
use crate::plugins::{
    PluginManager, PluginReturnKind, PluginSignature, extract_plugin_call_name,
    plugin_signature_map,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Boolean,
    Number,
    String,
    Array,
    Object,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
enum OperatorContractKind {
    SameKnownType,
    BothNumber,
    BothString,
    Contains,
}

#[derive(Debug, Clone, Copy)]
struct OperatorContract {
    rule_id: &'static str,
    kind: OperatorContractKind,
}

impl ValueKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
            Self::Unknown => "unknown",
        }
    }
}

fn operator_from_expression(expr: &str) -> Option<(&'static str, usize, usize)> {
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(idx) = expr.find(op) {
            return Some((op, idx, op.len()));
        }
    }

    for op in ["contains", "matches", "startsWith", "endsWith"] {
        let token = format!(" {} ", op);
        if let Some(idx) = expr.find(&token) {
            return Some((op, idx, token.len()));
        }
    }

    None
}

fn operator_contract(op: &str) -> Option<OperatorContract> {
    match op {
        "==" | "!=" => Some(OperatorContract {
            rule_id: "SEM_T001",
            kind: OperatorContractKind::SameKnownType,
        }),
        ">" | "<" | ">=" | "<=" => Some(OperatorContract {
            rule_id: "SEM_T002",
            kind: OperatorContractKind::BothNumber,
        }),
        "matches" | "startsWith" | "endsWith" => Some(OperatorContract {
            rule_id: "SEM_T003",
            kind: OperatorContractKind::BothString,
        }),
        "contains" => Some(OperatorContract {
            rule_id: "SEM_T004",
            kind: OperatorContractKind::Contains,
        }),
        _ => None,
    }
}

fn plugin_signatures() -> HashMap<String, PluginSignature> {
    plugin_signature_map()
}

fn section_content_line(start_line: usize, idx: usize) -> usize {
    start_line + idx + 2
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

fn infer_value_kind(expr: &str, signatures: &HashMap<String, PluginSignature>) -> ValueKind {
    let trimmed = expr.trim();

    if trimmed == "true" || trimmed == "false" {
        return ValueKind::Boolean;
    }

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return ValueKind::String;
    }

    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return ValueKind::Array;
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return ValueKind::Object;
    }

    if trimmed.parse::<f64>().is_ok() {
        return ValueKind::Number;
    }

    if let Some(plugin_name) = extract_plugin_call_name(trimmed) {
        return match signatures.get(plugin_name.as_str()).map(|s| s.return_kind) {
            Some(PluginReturnKind::Boolean) => ValueKind::Boolean,
            Some(PluginReturnKind::Number) => ValueKind::Number,
            Some(PluginReturnKind::String) => ValueKind::String,
            _ => ValueKind::Unknown,
        };
    }

    ValueKind::Unknown
}

fn detect_type_mismatch(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<AssertionTypeMismatch> {
    let (op, op_idx, op_len) = operator_from_expression(expr)?;
    let contract = operator_contract(op)?;
    let lhs = expr[..op_idx].trim();
    let rhs = expr[op_idx + op_len..].trim();
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }

    let lhs_kind = infer_value_kind(lhs, signatures);
    let rhs_kind = infer_value_kind(rhs, signatures);

    match contract.kind {
        OperatorContractKind::SameKnownType => {
            if lhs_kind != ValueKind::Unknown
                && rhs_kind != ValueKind::Unknown
                && lhs_kind != rhs_kind
            {
                return Some(AssertionTypeMismatch {
                    rule_id: contract.rule_id.to_string(),
                    line: 0,
                    expression: expr.to_string(),
                    message: format!(
                        "Type-incompatible comparison: {} is {}, but {} is {}",
                        lhs,
                        lhs_kind.as_str(),
                        rhs,
                        rhs_kind.as_str()
                    ),
                    expected: lhs_kind.as_str().to_string(),
                    actual: rhs_kind.as_str().to_string(),
                });
            }
        }
        OperatorContractKind::BothNumber => {
            for (side_expr, side_kind) in [(lhs, lhs_kind), (rhs, rhs_kind)] {
                if side_kind != ValueKind::Unknown && side_kind != ValueKind::Number {
                    return Some(AssertionTypeMismatch {
                        rule_id: contract.rule_id.to_string(),
                        line: 0,
                        expression: expr.to_string(),
                        message: format!(
                            "Ordering operator '{}' requires numbers, but {} is {}",
                            op,
                            side_expr,
                            side_kind.as_str()
                        ),
                        expected: "number".to_string(),
                        actual: side_kind.as_str().to_string(),
                    });
                }
            }
        }
        OperatorContractKind::BothString => {
            for (side_expr, side_kind) in [(lhs, lhs_kind), (rhs, rhs_kind)] {
                if side_kind != ValueKind::Unknown && side_kind != ValueKind::String {
                    return Some(AssertionTypeMismatch {
                        rule_id: contract.rule_id.to_string(),
                        line: 0,
                        expression: expr.to_string(),
                        message: format!(
                            "Operator '{}' requires strings, but {} is {}",
                            op,
                            side_expr,
                            side_kind.as_str()
                        ),
                        expected: "string".to_string(),
                        actual: side_kind.as_str().to_string(),
                    });
                }
            }
        }
        OperatorContractKind::Contains => {
            if lhs_kind == ValueKind::String
                && rhs_kind != ValueKind::Unknown
                && rhs_kind != ValueKind::String
            {
                return Some(AssertionTypeMismatch {
                    rule_id: contract.rule_id.to_string(),
                    line: 0,
                    expression: expr.to_string(),
                    message: format!(
                        "Operator 'contains' with string LHS requires string RHS, but {} is {}",
                        rhs,
                        rhs_kind.as_str()
                    ),
                    expected: "string".to_string(),
                    actual: rhs_kind.as_str().to_string(),
                });
            }
        }
    }

    None
}

pub fn validate_plugin_semantics_completeness() -> Vec<String> {
    let mut issues = Vec::new();
    for plugin in PluginManager::new().list() {
        let name = plugin.name().to_string();
        let sig = plugin.signature();

        if sig.return_kind == PluginReturnKind::Unknown {
            issues.push(format!("{}: return_kind is Unknown", name));
        }
        if sig.arg_names.is_empty() {
            issues.push(format!("{}: arg_names is empty", name));
        }
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
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some(mut mismatch) = detect_type_mismatch(trimmed, &signatures) {
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
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            for plugin_name in extract_plugin_calls(trimmed) {
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
        assert_eq!(mismatches[0].rule_id, "SEM_T003");
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
