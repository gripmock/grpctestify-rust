use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use apif_parser as parser;
use apif_parser::tokenizer::{TokenKind, tokenize_assertion};
use apif_plugins::{PluginSignature, TypeInfo};
use apif_utils::section_content_line;
use serde_json::Value as JsonValue;

pub mod structure;
pub use structure::{
    UnusedVariable, collect_unused_variables, preamble_section_order, unused_variable_message,
};

static EXTRA_PLUGIN_NAMES: OnceLock<HashSet<String>> = OnceLock::new();

pub fn register_extra_plugin_names(names: HashSet<String>) {
    let _ = EXTRA_PLUGIN_NAMES.set(names);
}

fn extra_plugin_names() -> &'static HashSet<String> {
    EXTRA_PLUGIN_NAMES.get_or_init(HashSet::new)
}

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
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || chars[end] == '_' || chars[end] == '.')
        {
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
    if tokens.len() == 1
        && let TokenKind::Ident(name) = &tokens[0].kind
        && name.starts_with('$')
    {
        let var_name = &name[1..];
        if let Some(var_type) = var_types.get(var_name) {
            return *var_type;
        }
    }

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
    let char_to_byte = |char_idx: usize| -> usize {
        expr.char_indices()
            .nth(char_idx)
            .map_or(expr.len(), |(b, _)| b)
    };
    let lhs = expr[..char_to_byte(op_idx)].trim();
    let rhs = expr[char_to_byte(op_idx + op_len)..].trim();
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }

    let lhs_tokens = tokenize_assertion(lhs);
    let rhs_tokens = tokenize_assertion(rhs);
    let lhs_type = infer_type_from_tokens(&lhs_tokens, signatures, var_types);
    let rhs_type = infer_type_from_tokens(&rhs_tokens, signatures, var_types);

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

    if (op == "==" || op == "!=")
        && lhs_type != TypeInfo::Any
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

fn types_compatible(a: TypeInfo, b: TypeInfo) -> bool {
    if a == b {
        return true;
    }
    if a.is_numeric() && b.is_numeric() {
        return true;
    }
    if a == TypeInfo::Time && b.is_numeric() || b == TypeInfo::Time && a.is_numeric() {
        return true;
    }
    if a.is_stringy() && b.is_stringy() {
        return true;
    }
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
    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
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
                    break;
                }
            }
        }
    }
    deprecated
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstantAssertion {
    pub rule_id: String,
    pub line: usize,
    pub expression: String,
    pub always: bool,
    pub message: String,
}

pub fn collect_constant_assertions(doc: &parser::GctfDocument) -> Vec<ConstantAssertion> {
    let mut constants = Vec::new();
    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }
        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };
            if trimmed.is_empty() {
                continue;
            }
            let Some(always) = constant_eq_result(&trimmed) else {
                continue;
            };
            constants.push(ConstantAssertion {
                rule_id: "SEM_C001".to_string(),
                line: section_content_line(section.start_line, idx),
                expression: trimmed.to_string(),
                always,
                message: format!(
                    "Assertion compares two literals and always evaluates to {always} — it never actually checks the response"
                ),
            });
        }
    }
    constants
}

#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateAssertion {
    pub rule_id: String,
    pub line: usize,
    pub first_line: usize,
    pub expression: String,
    pub message: String,
}

pub fn collect_duplicate_assertions(doc: &parser::GctfDocument) -> Vec<DuplicateAssertion> {
    let mut duplicates = Vec::new();
    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };
            if trimmed.is_empty() {
                continue;
            }
            let this_line = section_content_line(section.start_line, idx);
            if let Some(&first_line) = seen.get(&trimmed) {
                duplicates.push(DuplicateAssertion {
                    rule_id: "SEM_C002".to_string(),
                    line: this_line,
                    first_line,
                    expression: trimmed,
                    message: format!(
                        "Duplicate assertion — identical to the one at line {first_line}"
                    ),
                });
            } else {
                seen.insert(trimmed, this_line);
            }
        }
    }
    duplicates
}

#[derive(Debug, Clone, PartialEq)]
pub struct RedundantResponseAssertion {
    pub rule_id: String,
    pub line: usize,
    pub expression: String,
    pub message: String,
}

pub fn collect_redundant_response_assertions(
    doc: &parser::GctfDocument,
) -> Vec<RedundantResponseAssertion> {
    let mut redundant = Vec::new();
    for d in doc.iter_chain() {
        for (i, section) in d.sections.iter().enumerate() {
            if section.section_type != parser::ast::SectionType::Response
                || !section.inline_options.with_asserts
                || section.inline_options.tolerance.is_some()
                || !section.inline_options.redact.is_empty()
                || section.inline_options.unordered_arrays
            {
                continue;
            }
            let Some(asserts_section) = d.sections.get(i + 1) else {
                continue;
            };
            if asserts_section.section_type != parser::ast::SectionType::Asserts {
                continue;
            }
            let parser::ast::SectionContent::Json(body) = &section.content else {
                continue;
            };
            let mut pinned = HashMap::new();
            flatten_scalar_paths(body, String::new(), &mut pinned);
            if pinned.is_empty() {
                continue;
            }

            for (idx, line) in asserts_section.raw_content.lines().enumerate() {
                let trimmed = match parser::assertions::strip_assertion_comments(line) {
                    Some(t) => t,
                    None => continue,
                };
                if trimmed.is_empty() {
                    continue;
                }
                let Some(path) = redundant_equality_path(&trimmed, &pinned) else {
                    continue;
                };
                redundant.push(RedundantResponseAssertion {
                    rule_id: "SEM_C003".to_string(),
                    line: section_content_line(asserts_section.start_line, idx),
                    expression: trimmed,
                    message: format!(
                        "`.{path}` is already pinned to this exact value by RESPONSE — this assertion can never catch anything RESPONSE matching wouldn't already fail on"
                    ),
                });
            }
        }
    }
    redundant
}

fn flatten_scalar_paths(value: &JsonValue, prefix: String, out: &mut HashMap<String, JsonValue>) {
    match value {
        JsonValue::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_scalar_paths(v, path, out);
            }
        }
        JsonValue::String(s) if s == "*" => {}
        JsonValue::Array(_) => {}
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix, value.clone());
            }
        }
    }
}

fn redundant_equality_path(expr: &str, pinned: &HashMap<String, JsonValue>) -> Option<String> {
    let ast = parser::parse_assertion(expr);
    let parser::AssertionExpr::Binary { op, left, right } = ast else {
        return None;
    };
    if op != parser::BinaryOp::Eq {
        return None;
    }
    let (path, lit) = match (*left, *right) {
        (
            parser::AssertionExpr::Atom(parser::Expr::JqPath(p)),
            parser::AssertionExpr::Atom(parser::Expr::Literal(l)),
        ) => (p, l),
        (
            parser::AssertionExpr::Atom(parser::Expr::Literal(l)),
            parser::AssertionExpr::Atom(parser::Expr::JqPath(p)),
        ) => (p, l),
        _ => return None,
    };
    let path = path.strip_prefix('.')?;
    if path.is_empty()
        || !path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return None;
    }
    let pinned_value = pinned.get(path)?;
    if literal_matches_json(&lit, pinned_value) {
        Some(path.to_string())
    } else {
        None
    }
}

fn literal_matches_json(lit: &parser::Literal, value: &JsonValue) -> bool {
    use parser::Literal;
    match (lit, value) {
        (Literal::Bool(b), JsonValue::Bool(v)) => b == v,
        (Literal::Str(s), JsonValue::String(v)) => s == v,
        (Literal::Null, JsonValue::Null) => true,
        (Literal::Number(n), JsonValue::Number(v)) => n
            .parse::<f64>()
            .is_ok_and(|nf| v.as_f64().is_some_and(|vf| nf == vf)),
        _ => false,
    }
}

fn constant_eq_result(expr: &str) -> Option<bool> {
    let ast = parser::parse_assertion(expr);
    let parser::AssertionExpr::Binary { op, left, right } = ast else {
        return None;
    };
    if !matches!(op, parser::BinaryOp::Eq | parser::BinaryOp::Ne) {
        return None;
    }
    let parser::AssertionExpr::Atom(parser::Expr::Literal(lhs)) = *left else {
        return None;
    };
    let parser::AssertionExpr::Atom(parser::Expr::Literal(rhs)) = *right else {
        return None;
    };
    let equal = literals_equal(&lhs, &rhs)?;
    Some(if op == parser::BinaryOp::Eq {
        equal
    } else {
        !equal
    })
}

fn literals_equal(a: &parser::Literal, b: &parser::Literal) -> Option<bool> {
    use parser::Literal;
    match (a, b) {
        (Literal::Bool(x), Literal::Bool(y)) => Some(x == y),
        (Literal::Str(x), Literal::Str(y)) => Some(x == y),
        (Literal::Null, Literal::Null) => Some(true),
        (Literal::Number(x), Literal::Number(y)) => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => Some(x == y),
            _ => None,
        },
        _ => None,
    }
}

pub fn collect_assertion_type_mismatches(doc: &parser::GctfDocument) -> Vec<AssertionTypeMismatch> {
    let signatures = plugin_signatures();
    let var_types = extract_variable_types(doc);
    let mut mismatches = Vec::new();

    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
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
    collect_unknown_plugin_calls_with_extra(doc, extra_plugin_names())
}

pub fn collect_unknown_plugin_calls_with_extra(
    doc: &parser::GctfDocument,
    extra_known: &std::collections::HashSet<String>,
) -> Vec<UnknownPluginCall> {
    let signatures = plugin_signatures();
    let mut known_plugins: Vec<String> = signatures
        .keys()
        .cloned()
        .chain(extra_known.iter().cloned())
        .collect();
    known_plugins.sort();

    let mut unknown = Vec::new();

    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = match parser::assertions::strip_assertion_comments(line) {
                Some(t) => t,
                None => continue,
            };

            for plugin_name in extract_plugin_calls(&trimmed) {
                if signatures.contains_key(plugin_name.as_str())
                    || extra_known.contains(&plugin_name)
                {
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
    fn semantics_detects_boolean_vs_number() {
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
    fn semantics_multibyte_expression_no_panic() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n\u{ef}\u{ef} == 1\n.na\u{ef}ve == \"x\"\n";

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let _ = collect_assertion_type_mismatches(&doc);

        let mismatch =
            detect_type_mismatch(".na\u{ef}ve == \"x\"", plugin_signatures(), &HashMap::new());
        assert!(mismatch.is_none(), "got: {:?}", mismatch);
    }

    #[test]
    fn semantics_allows_boolean_compare() {
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
    fn semantics_detects_startswith_non_string() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.names) startsWith "a"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn semantics_detects_unknown_plugin_calls() {
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
        assert!(unknown[0].suggestion.is_some());
    }

    #[test]
    fn semantics_allows_known_plugin_calls() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@regex(.name, "^a") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let unknown = collect_unknown_plugin_calls(&doc);
        assert!(unknown.is_empty());
    }

    #[test]
    fn semantics_type_cast_number_allows_ordering() {
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
    fn semantics_type_cast_string_allows_contains() {
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
    fn semantics_type_cast_uint_allows_ordering() {
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
    fn semantics_type_cast_bool_allows_equal() {
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
    fn semantics_type_cast_rejects_bool_ordering() {
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
    fn semantics_type_cast_rejects_string_ordering() {
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
    fn semantics_all_types_cast() {
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
    fn semantics_type_cast_without_annotation_passes() {
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

    #[test]
    fn extract_variable_types_simple() {
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
    fn extract_variable_types_without_type_annotation() {
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
    fn variable_type_in_assertion() {
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
    fn variable_type_without_annotation_passes() {
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
    fn variable_type_string_contains() {
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
    fn time_type_ordering() {
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
    fn time_type_rejects_string_ops() {
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
    fn time_variable_type_in_assertion() {
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
    fn bracket_path_with_dot_index() {
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
    fn bracket_path_with_string_key() {
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
    fn collect_constant_assertions_always_true() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
"SERVING" == "SERVING"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let constants = collect_constant_assertions(&doc);
        assert_eq!(constants.len(), 1);
        assert_eq!(constants[0].rule_id, "SEM_C001");
        assert!(constants[0].always);
    }

    #[test]
    fn collect_constant_assertions_always_false() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
1 == 2
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let constants = collect_constant_assertions(&doc);
        assert_eq!(constants.len(), 1);
        assert!(!constants[0].always);
    }

    #[test]
    fn collect_constant_assertions_numeric_equivalence() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
1 == 1.0
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let constants = collect_constant_assertions(&doc);
        assert_eq!(constants.len(), 1);
        assert!(constants[0].always);
    }

    #[test]
    fn collect_constant_assertions_ignores_field_comparisons() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.status == "SERVING"
$name == "SERVING"
@len(.items) == 3
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let constants = collect_constant_assertions(&doc);
        assert!(
            constants.is_empty(),
            "field/variable/plugin comparisons must never be flagged as constant: {constants:?}"
        );
    }

    #[test]
    fn collect_duplicate_assertions_flags_exact_repeat() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.status == "SERVING"
.count > 0
.status == "SERVING"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let dups = collect_duplicate_assertions(&doc);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].rule_id, "SEM_C002");
        assert_eq!(dups[0].expression, ".status == \"SERVING\"");
    }

    #[test]
    fn collect_duplicate_assertions_ignores_distinct_lines() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.status == "SERVING"
.count > 0
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(collect_duplicate_assertions(&doc).is_empty());
    }

    #[test]
    fn collect_duplicate_assertions_ignores_trailing_comment_difference() {
        let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n.status == \"SERVING\"\n.status == \"SERVING\" // same check\n";
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let dups = collect_duplicate_assertions(&doc);
        assert_eq!(dups.len(), 1);
    }

    #[test]
    fn collect_deprecated_plugin_uuid() {
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
    fn collect_deprecated_plugin_email() {
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
    fn collect_deprecated_plugin_empty() {
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
    fn collect_deprecated_plugin_finds_second_document_in_chain() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.ok == true

--- ENDPOINT ---
test.Service/Method2

--- ASSERTS ---
@uuid(.id) == true
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(!doc.is_single_document(), "fixture must actually chain");
        let deprecated = collect_deprecated_plugin_calls(&doc);
        assert_eq!(deprecated.len(), 1);
        assert_eq!(deprecated[0].plugin_name, "uuid");
    }

    #[test]
    fn collect_unknown_plugin_calls_finds_second_document_in_chain() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.ok == true

--- ENDPOINT ---
test.Service/Method2

--- ASSERTS ---
@totally_made_up(.id)
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(!doc.is_single_document(), "fixture must actually chain");
        let unknown = collect_unknown_plugin_calls(&doc);
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].plugin_name, "totally_made_up");
    }

    #[test]
    fn collect_assertion_type_mismatches_finds_second_document_in_chain() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.ok == true

--- ENDPOINT ---
test.Service/Method2

--- ASSERTS ---
@len(.names) startsWith "a"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(!doc.is_single_document(), "fixture must actually chain");
        let mismatches = collect_assertion_type_mismatches(&doc);
        assert_eq!(mismatches.len(), 1, "{mismatches:?}");
        assert_eq!(mismatches[0].rule_id, "SEM_T005");
    }

    #[test]
    fn collect_deprecated_plugin_skips_canonical() {
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

    #[test]
    fn collect_redundant_response_assertions_flags_exact_pinned_field() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE with_asserts ---
{
  "status": "ok",
  "count": 5
}

--- ASSERTS ---
.status == "ok"
.count > 0
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let redundant = collect_redundant_response_assertions(&doc);
        assert_eq!(redundant.len(), 1, "got: {:?}", redundant);
        assert_eq!(redundant[0].rule_id, "SEM_C003");
        assert_eq!(redundant[0].expression, r#".status == "ok""#);
    }

    #[test]
    fn collect_redundant_response_assertions_ignores_different_value() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE with_asserts ---
{
  "status": "ok"
}

--- ASSERTS ---
.status == "different"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(collect_redundant_response_assertions(&doc).is_empty());
    }

    #[test]
    fn collect_redundant_response_assertions_ignores_field_comparison() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE with_asserts ---
{
  "status": "ok",
  "echo": "ok"
}

--- ASSERTS ---
.status == .echo
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(collect_redundant_response_assertions(&doc).is_empty());
    }

    #[test]
    fn collect_redundant_response_assertions_ignores_wildcard_field() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE with_asserts ---
{
  "id": "*"
}

--- ASSERTS ---
.id == "*"
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(collect_redundant_response_assertions(&doc).is_empty());
    }

    #[test]
    fn collect_redundant_response_assertions_ignores_without_with_asserts() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE ---
{
  "status": "ok"
}
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(collect_redundant_response_assertions(&doc).is_empty());
    }

    #[test]
    fn collect_redundant_response_assertions_ignores_tolerance() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- RESPONSE with_asserts tolerance=0.01 ---
{
  "score": 1.5
}

--- ASSERTS ---
.score == 1.5
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(
            collect_redundant_response_assertions(&doc).is_empty(),
            "tolerance weakens what RESPONSE pins — must not flag"
        );
    }
}
