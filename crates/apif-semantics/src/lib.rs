use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use apif_parser as parser;
use apif_parser::tokenizer::{TokenKind, tokenize_assertion};
use apif_plugins::{PluginSignature, TypeInfo};
use apif_utils::section_content_line;

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
    use apif_plugins::PLUGIN_SIGNATURES;
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
        // Match plugin name including dotted type.method syntax
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || chars[end] == '_' || chars[end] == '.')
        {
            end += 1;
        }

        if end == start {
            i += 1;
            continue;
        }

        // Skip whitespace before (
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

/// Extract variable type annotations from EXTRACT sections across a document chain.
/// Returns a map of variable name → TypeInfo parsed from `name:Type = .path` lines.
pub fn extract_variable_types(doc: &parser::GctfDocument) -> HashMap<String, TypeInfo> {
    let mut var_types = HashMap::new();
    for d in doc.iter_chain() {
        for section in &d.sections {
            if section.section_type != parser::ast::SectionType::Extract {
                continue;
            }
            for line in section.raw_content.lines() {
                if let Some((name, Some(type_name), _)) =
                    parser::gctf_tokenizer::tokenize_extract_line_full(line)
                    && let Some(ti) = TypeInfo::parse_type_name(&type_name)
                {
                    var_types.insert(name, ti);
                }
            }
        }
    }
    var_types
}

pub fn infer_type_from_tokens(
    tokens: &[parser::tokenizer::Token],
    signatures: &HashMap<String, PluginSignature>,
    var_types: &HashMap<String, TypeInfo>,
) -> TypeInfo {
    // Check for $var_name pattern — look up variable type
    if tokens.len() == 1
        && let TokenKind::Ident(name) = &tokens[0].kind
        && name.starts_with('$')
    {
        let var_name = &name[1..];
        if let Some(var_type) = var_types.get(var_name) {
            return *var_type;
        }
    }

    // Check for `:TypeName` type annotation: `expr:number`
    if tokens.len() >= 2
        && let Some(TokenKind::Ident(name)) = tokens.last().map(|t| &t.kind)
        && let Some(cast_type) = TypeInfo::parse_type_name(name)
        && tokens[tokens.len() - 2].kind == TokenKind::Colon
    {
        return cast_type;
    }

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

    if tokens.len() >= 3 && matches!(&tokens[0].kind, TokenKind::At) {
        let name = match &tokens[1].kind {
            TokenKind::Ident(s) => s.as_str(),
            _ => "",
        };
        if !name.is_empty() {
            // Check for @type.method syntax — look up full name including method
            let full_name: String = if tokens.len() >= 5
                && matches!(&tokens[2].kind, TokenKind::Dot)
                && let TokenKind::Ident(method) = &tokens[3].kind
            {
                format!("{}.{}", name, method)
            } else {
                name.to_string()
            };
            if let Some(sig) = signatures.get(full_name.as_str()) {
                return sig.return_type;
            }
        }
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
    var_types: &HashMap<String, TypeInfo>,
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
    let lhs_type = infer_type_from_tokens(&lhs_tokens, signatures, var_types);
    let rhs_type = infer_type_from_tokens(&rhs_tokens, signatures, var_types);

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

    if matches!(op, ">" | "<" | ">=" | "<=")
        && !rhs_type.is_numeric()
        && !rhs_type.is_stringy()
        && rhs_type != TypeInfo::Any
        && lhs_type != TypeInfo::Time
    {
        return Some(AssertionTypeMismatch {
            rule_id: "SEM_T002".to_string(),
            line: 0,
            expression: expr.to_string(),
            message: format!(
                "Ordering operator '{}' requires a number or time string on the right, but {} is {}",
                op,
                rhs,
                rhs_type.display_name()
            ),
            expected: "number or string".to_string(),
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
    // Time is compatible with numeric (both support ordering)
    if a == TypeInfo::Time && b.is_numeric() || b == TypeInfo::Time && a.is_numeric() {
        return true;
    }
    // String-like types are compatible
    if a.is_stringy() && b.is_stringy() {
        return true;
    }
    // Unknown (Any) is compatible with anything
    if a == TypeInfo::Any || b == TypeInfo::Any {
        return true;
    }
    false
}

const DEPRECATED_PLUGINS: &[(&str, &str)] = &[
    ("uuid", "is_uuid"),
    ("email", "is_email"),
    ("ip", "is_ip"),
    ("url", "is_url"),
    ("timestamp", "is_timestamp"),
    ("empty", "is_empty"),
    ("scope_message_count", "scope.message_count"),
    ("scope_index", "scope.index"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecatedPluginCall {
    pub rule_id: String,
    pub line: usize,
    pub expression: String,
    pub plugin_name: String,
    pub message: String,
    pub replacement: String,
}

pub fn collect_deprecated_plugin_calls(doc: &parser::GctfDocument) -> Vec<DeprecatedPluginCall> {
    let mut deprecated = Vec::new();
    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }
        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };
            for (old_name, new_name) in DEPRECATED_PLUGINS {
                let old_call = format!("@{}(", old_name);
                if trimmed.contains(&old_call) {
                    let _name_start = trimmed.find(&old_call).unwrap();
                    deprecated.push(DeprecatedPluginCall {
                        rule_id: "SEM_D001".to_string(),
                        line: section_content_line(section.start_line, idx),
                        expression: trimmed.to_string(),
                        plugin_name: old_name.to_string(),
                        message: format!(
                            "'@{}' is deprecated, use '@{}' instead",
                            old_name, new_name
                        ),
                        replacement: format!("@{}", new_name),
                    });
                    break; // one warning per line
                }
            }
        }
    }
    deprecated
}

pub fn collect_assertion_type_mismatches(doc: &parser::GctfDocument) -> Vec<AssertionTypeMismatch> {
    let signatures = plugin_signatures();
    let var_types = extract_variable_types(doc);
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

            if let Some(mut mismatch) = detect_type_mismatch(&trimmed, signatures, &var_types) {
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
        // Suggestion now points to the closest type method match
        assert!(unknown[0].suggestion.is_some());
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

    // ─── Type cast semantics tests ────────────────────────────────────

    #[test]
    fn test_semantics_type_cast_number_allows_ordering() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.price:number >= 0
.price:number > 0
.price:number <= 100
.price:number < 200
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_semantics_type_cast_string_allows_contains() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.name:string contains "hello"
.name:string startsWith "he"
.name:string endsWith "lo"
.name:string matches "^he.*lo$"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_semantics_type_cast_uint_allows_ordering() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items):uint >= 0
@len(.items):uint > 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_semantics_type_cast_bool_allows_equal() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.active:bool == true
.active:bool != false
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_semantics_type_cast_rejects_bool_ordering() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.active:bool > 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn test_semantics_type_cast_rejects_string_ordering() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.name:string >= "a"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn test_semantics_all_types_cast() {
        let cases = [
            ("bool", "true"),
            ("uint", "0"),
            ("number", "0"),
            ("string", "\"\""),
            ("json", "null"),
            ("yaml", "null"),
            ("uuid", "\"\""),
            ("email", "\"\""),
            ("url", "\"\""),
            ("ip", "\"\""),
        ];
        for (type_name, rhs) in &cases {
            let content = format!(
                r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.x:{} == {}
"#,
                type_name, rhs
            );
            let doc = parser::parse_gctf_from_str(&content, "test.gctf").unwrap();
            let mismatches = collect_assertion_type_mismatches(&doc);
            assert!(
                mismatches.is_empty(),
                "Failed for type cast ':{}': {:?}",
                type_name,
                mismatches
            );
        }
    }

    #[test]
    fn test_semantics_type_cast_without_annotation_passes() {
        // Any now supports all operators — no annotation needed for ordering
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.price >= 0
.price > 0
.price <= 0
.price < 0
.name contains "hello"
.name startsWith "h"
@len(.items) > 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Any type should allow all operators, got: {:?}",
            mismatches
        );
    }

    // ─── Variable type tracking tests ─────────────────────────────────

    #[test]
    fn test_extract_variable_types_simple() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"price": 42}

--- EXTRACT ---
total:number = .price
name:string = .user.name
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let var_types = extract_variable_types(&doc);
        assert_eq!(var_types.len(), 2);
        assert_eq!(var_types.get("total"), Some(&TypeInfo::Number));
        assert_eq!(var_types.get("name"), Some(&TypeInfo::String));
    }

    #[test]
    fn test_extract_variable_types_without_type_annotation() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"price": 42}

--- EXTRACT ---
total = .price
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let var_types = extract_variable_types(&doc);
        assert!(
            var_types.is_empty(),
            "No type annotations should yield empty map"
        );
    }

    #[test]
    fn test_variable_type_in_assertion() {
        // When {{price}} is used and its type is known from EXTRACT,
        // ordering operators should be allowed
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"price": 42}

--- EXTRACT ---
price:number = .price

--- ASSERTS ---
$price >= 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches for typed $var with ordering op, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_variable_type_without_annotation_passes() {
        // Any supports all operators — untyped $var allows ordering
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"price": 42}

--- EXTRACT ---
price = .price

--- ASSERTS ---
$price >= 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Any type $var should allow ordering, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_variable_type_string_contains() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"name": "hello"}

--- EXTRACT ---
user_name:string = .name

--- ASSERTS ---
$user_name contains "hello"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Expected no mismatches for typed $var with string op, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_time_type_ordering() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.created_at:time >= "2024-01-01"
.expires_at:timestamp > "2025-01-01"
.duration:duration < "30s"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Time type should allow ordering, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_time_type_rejects_string_ops() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.created_at:time contains "2024"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn test_time_variable_type_in_assertion() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- REQUEST ---
{}

--- RESPONSE ---
{"ts": "2024-06-15T10:00:00Z"}

--- EXTRACT ---
created:time = .ts

--- ASSERTS ---
$created > "2024-01-01"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Time typed $var should allow ordering, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_bracket_path_with_dot_index() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.x[.idx]:number >= 0
.x[.idx]:string contains "hello"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Bracket path with .var index should allow typed ops, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_bracket_path_with_string_key() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.ips_to_decorations["10.0.0.1"].environment == "production"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert!(
            mismatches.is_empty(),
            "Bracket path with string key roundtrip, got: {:?}",
            mismatches
        );
    }

    #[test]
    fn test_collect_deprecated_plugin_uuid() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@uuid(.id) == true
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let deprecated = collect_deprecated_plugin_calls(&doc);
        assert_eq!(deprecated.len(), 1);
        assert_eq!(deprecated[0].rule_id, "SEM_D001");
        assert_eq!(deprecated[0].plugin_name, "uuid");
        assert!(deprecated[0].message.contains("is_uuid"));
    }

    #[test]
    fn test_collect_deprecated_plugin_email() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@email(.addr) == true
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let deprecated = collect_deprecated_plugin_calls(&doc);
        assert_eq!(deprecated.len(), 1);
        assert_eq!(deprecated[0].plugin_name, "email");
    }

    #[test]
    fn test_collect_deprecated_plugin_empty() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@empty(.name)
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let deprecated = collect_deprecated_plugin_calls(&doc);
        assert_eq!(deprecated.len(), 1);
        assert_eq!(deprecated[0].plugin_name, "empty");
    }

    #[test]
    fn test_collect_deprecated_plugin_skips_canonical() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@is_uuid(.id) == true
@is_empty(.name)
@is_email(.addr)
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let deprecated = collect_deprecated_plugin_calls(&doc);
        assert!(
            deprecated.is_empty(),
            "Canonical names should not be flagged, got: {:?}",
            deprecated
        );
    }
}
