use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::parser;
use crate::plugins::{
    PluginReturnKind, PluginSignature, extract_plugin_call_name, plugin_signature_map,
};

#[derive(Debug, Clone, Copy)]
struct RewriteRuleMetadata {
    id: &'static str,
    preconditions: &'static str,
    negative_cases: &'static str,
    proof_note: &'static str,
}

const REWRITE_RULES: &[RewriteRuleMetadata] = &[
    RewriteRuleMetadata {
        id: "OPT_B001",
        preconditions: "lhs is boolean plugin expr and rhs is true",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: expr == true is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: "OPT_B002",
        preconditions: "lhs is boolean plugin expr and rhs is false",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: expr == false is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: "OPT_B003",
        preconditions: "lhs is true and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: true == expr is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: "OPT_B004",
        preconditions: "lhs is false and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: false == expr is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: "OPT_B005",
        preconditions: "expression has form !!<bool-plugin-expr>",
        negative_cases: "inner expr is not proven boolean-safe",
        proof_note: "Double negation elimination for boolean expressions",
    },
    RewriteRuleMetadata {
        id: "OPT_B006",
        preconditions: "binary compare over two literals only",
        negative_cases: "contains non-literals, dynamic plugin calls, or unknown values",
        proof_note: "Constant folding preserves comparison result",
    },
    RewriteRuleMetadata {
        id: "OPT_B007",
        preconditions: "expression has form x == x and x is idempotent",
        negative_cases: "x may be non-idempotent or side-effectful",
        proof_note: "Reflexive equality over idempotent expressions is always true",
    },
    RewriteRuleMetadata {
        id: "OPT_B008",
        preconditions: "expression has form x != x and x is idempotent",
        negative_cases: "x may be non-idempotent or side-effectful",
        proof_note: "Reflexive inequality over idempotent expressions is always false",
    },
    RewriteRuleMetadata {
        id: "OPT_N001",
        preconditions: "operator alias startswith/endswith is present",
        negative_cases: "already canonicalized form",
        proof_note: "Canonical spelling rewrite preserves operator semantics",
    },
];

fn rule_metadata(rule_id: &str) -> Option<&'static RewriteRuleMetadata> {
    REWRITE_RULES.iter().find(|r| r.id == rule_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationHint {
    pub rule_id: String,
    pub line: usize,
    pub before: String,
    pub after: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preconditions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_cases: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_note: Option<String>,
}

fn build_hint(rule_id: &str, line: usize, before: &str, after: String) -> OptimizationHint {
    let meta = rule_metadata(rule_id);
    OptimizationHint {
        rule_id: rule_id.to_string(),
        line,
        before: before.to_string(),
        after,
        preconditions: meta.map(|m| m.preconditions.to_string()),
        negative_cases: meta.map(|m| m.negative_cases.to_string()),
        proof_note: meta.map(|m| m.proof_note.to_string()),
    }
}

fn plugin_signatures() -> HashMap<String, PluginSignature> {
    plugin_signature_map()
}

fn section_content_line(start_line: usize, idx: usize) -> usize {
    start_line + idx + 2
}

fn boolean_plugins() -> HashSet<String> {
    plugin_signatures()
        .into_iter()
        .filter(|(_, signature)| {
            signature.return_kind == PluginReturnKind::Boolean
                && signature.safe_for_rewrite
                && signature.deterministic
                && signature.idempotent
        })
        .map(|(name, _)| name)
        .collect()
}

fn is_boolean_plugin_expr(expr: &str, bool_plugins: &HashSet<String>) -> bool {
    let Some(plugin_name) = extract_plugin_call_name(expr) else {
        return false;
    };

    bool_plugins.contains(plugin_name.as_str())
}

fn suggest_boolean_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
) -> Option<(&'static str, String)> {
    let (lhs, rhs) = expr.split_once("==")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "true" {
        return Some(("OPT_B001", lhs.to_string()));
    }
    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "false" {
        return Some(("OPT_B002", format!("!{}", lhs)));
    }
    if lhs == "true" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some(("OPT_B003", rhs.to_string()));
    }
    if lhs == "false" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some(("OPT_B004", format!("!{}", rhs)));
    }

    None
}

fn suggest_double_negation_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
) -> Option<(&'static str, String)> {
    let trimmed = expr.trim();
    if !trimmed.starts_with("!!") {
        return None;
    }

    let inner = trimmed
        .trim_start_matches('!')
        .trim_start_matches('!')
        .trim();
    if is_boolean_plugin_expr(inner, bool_plugins) {
        return Some(("OPT_B005", inner.to_string()));
    }

    None
}

fn suggest_operator_canonicalization(expr: &str) -> Option<(&'static str, String)> {
    let mut rewritten = expr.to_string();
    let mut changed = false;

    for (from, to) in [
        (" startswith ", " startsWith "),
        (" endswith ", " endsWith "),
    ] {
        if rewritten.contains(from) {
            rewritten = rewritten.replace(from, to);
            changed = true;
        }
    }

    if changed {
        Some(("OPT_N001", rewritten))
    } else {
        None
    }
}

fn parse_literal(expr: &str) -> Option<serde_json::Value> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "true" || trimmed == "false" || trimmed == "null" {
        return serde_json::from_str(trimmed).ok();
    }

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return serde_json::from_str(trimmed).ok();
    }

    if trimmed.parse::<f64>().is_ok() {
        return serde_json::from_str(trimmed).ok();
    }

    None
}

fn suggest_constant_folding(expr: &str) -> Option<(&'static str, String)> {
    let operators = ["==", "!=", ">=", "<=", ">", "<"];
    for op in operators {
        let Some(idx) = expr.find(op) else {
            continue;
        };

        let lhs_raw = expr[..idx].trim();
        let rhs_raw = expr[idx + op.len()..].trim();
        if lhs_raw.is_empty() || rhs_raw.is_empty() {
            continue;
        }

        let Some(lhs) = parse_literal(lhs_raw) else {
            continue;
        };
        let Some(rhs) = parse_literal(rhs_raw) else {
            continue;
        };

        let folded = match op {
            "==" => Some(lhs == rhs),
            "!=" => Some(lhs != rhs),
            ">" => Some(lhs.as_f64()? > rhs.as_f64()?),
            "<" => Some(lhs.as_f64()? < rhs.as_f64()?),
            ">=" => Some(lhs.as_f64()? >= rhs.as_f64()?),
            "<=" => Some(lhs.as_f64()? <= rhs.as_f64()?),
            _ => None,
        }?;

        return Some(("OPT_B006", folded.to_string()));
    }

    None
}

fn is_idempotent_expr(expr: &str, signatures: &HashMap<String, PluginSignature>) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }

    if parse_literal(trimmed).is_some() {
        return true;
    }

    if (trimmed.starts_with("{{") && trimmed.ends_with("}}")) || trimmed.starts_with('.') {
        return true;
    }

    if trimmed.starts_with('(') && trimmed.ends_with(')') && trimmed.len() >= 2 {
        return is_idempotent_expr(&trimmed[1..trimmed.len() - 1], signatures);
    }

    if let Some(plugin_name) = extract_plugin_call_name(trimmed) {
        return signatures
            .get(plugin_name.as_str())
            .map(|sig| sig.idempotent)
            .unwrap_or(false);
    }

    false
}

fn suggest_reflexive_idempotent_equality(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<(&'static str, String)> {
    let (lhs, rhs) = expr.split_once("==")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if lhs.is_empty() || rhs.is_empty() || lhs != rhs {
        return None;
    }

    if parse_literal(lhs).is_some() && parse_literal(rhs).is_some() {
        return None;
    }

    if is_idempotent_expr(lhs, signatures) {
        Some(("OPT_B007", "true".to_string()))
    } else {
        None
    }
}

fn suggest_reflexive_idempotent_inequality(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<(&'static str, String)> {
    let (lhs, rhs) = expr.split_once("!=")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if lhs.is_empty() || rhs.is_empty() || lhs != rhs {
        return None;
    }

    if parse_literal(lhs).is_some() && parse_literal(rhs).is_some() {
        return None;
    }

    if is_idempotent_expr(lhs, signatures) {
        Some(("OPT_B008", "false".to_string()))
    } else {
        None
    }
}

fn rewrite_assertion_expression_with_context(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    bool_plugins: &HashSet<String>,
) -> Option<(&'static str, String)> {
    if let Some((rule_id, rewrite)) = suggest_boolean_rewrite(expr, bool_plugins) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_double_negation_rewrite(expr, bool_plugins) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_operator_canonicalization(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_constant_folding(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_reflexive_idempotent_equality(expr, signatures) {
        return Some((rule_id, rewrite));
    }

    suggest_reflexive_idempotent_inequality(expr, signatures)
}

pub fn rewrite_assertion_expression(expr: &str) -> Option<(&'static str, String)> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    rewrite_assertion_expression_with_context(expr, &signatures, &bool_plugins)
}

pub fn rewrite_assertion_expression_fixed_point(expr: &str) -> String {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();

    let mut current = expr.trim().to_string();
    for _ in 0..32 {
        let Some((_, rewritten)) =
            rewrite_assertion_expression_with_context(&current, &signatures, &bool_plugins)
        else {
            break;
        };

        let normalized = rewritten.trim().to_string();
        if normalized == current {
            break;
        }
        current = normalized;
    }

    current
}

pub fn collect_assertion_optimizations(doc: &parser::GctfDocument) -> Vec<OptimizationHint> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    let mut hints = Vec::new();

    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some((rule_id, rewrite)) =
                rewrite_assertion_expression_with_context(trimmed, &signatures, &bool_plugins)
            {
                debug_assert!(rule_metadata(rule_id).is_some());
                hints.push(build_hint(
                    rule_id,
                    section_content_line(section.start_line, idx),
                    trimmed,
                    rewrite,
                ));
            }
        }
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_assertion_optimizations_detects_boolean_rewrite() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x-request-id") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B001");
        assert_eq!(hints[0].after, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn test_collect_assertion_optimizations_detects_double_negation_rewrite() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@has_header("x-request-id")
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B005");
        assert_eq!(hints[0].after, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn test_collect_assertion_optimizations_detects_operator_canonicalization() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.name startswith "abc"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_N001");
        assert_eq!(hints[0].after, ".name startsWith \"abc\"");
    }

    #[test]
    fn test_collect_assertion_optimizations_no_double_negation_for_non_boolean_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@len(.items)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert!(hints.is_empty());
    }

    #[test]
    fn test_collect_assertion_optimizations_constant_fold_numeric_compare() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
1 + 1 == 2
3 > 2
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);

        // Only '3 > 2' is a strict literal compare and safe to fold here.
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B006");
        assert_eq!(hints[0].before, "3 > 2");
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn test_collect_assertion_optimizations_constant_fold_string_equality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
"a" == "a"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B006");
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn test_rewrite_rule_metadata_is_complete() {
        let expected = [
            "OPT_B001", "OPT_B002", "OPT_B003", "OPT_B004", "OPT_B005", "OPT_B006", "OPT_B007",
            "OPT_B008", "OPT_N001",
        ];

        for id in expected {
            let meta = rule_metadata(id).unwrap_or_else(|| panic!("missing metadata for {id}"));
            assert!(!meta.preconditions.is_empty());
            assert!(!meta.negative_cases.is_empty());
            assert!(!meta.proof_note.is_empty());
        }
    }

    #[test]
    fn test_optimization_hint_contains_rule_metadata() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].preconditions.as_deref().is_some());
        assert!(hints[0].negative_cases.as_deref().is_some());
        assert!(hints[0].proof_note.as_deref().is_some());
    }

    #[test]
    fn test_collect_assertion_optimizations_reflexive_idempotent_path() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.user.id == .user.id
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B007");
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn test_collect_assertion_optimizations_no_reflexive_for_non_idempotent_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@env("HOME") == @env("HOME")
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);

        assert!(hints.is_empty());
    }

    #[test]
    fn test_collect_assertion_optimizations_reflexive_idempotent_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
{{ user_id }} != {{ user_id }}
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B008");
        assert_eq!(hints[0].after, "false");
    }

    #[test]
    fn test_rewrite_assertion_expression_fixed_point() {
        let expr = "true == @has_header(\"x-request-id\")";
        let rewritten = rewrite_assertion_expression_fixed_point(expr);
        assert_eq!(rewritten, "@has_header(\"x-request-id\")");
    }
}
