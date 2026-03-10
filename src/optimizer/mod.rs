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
    RewriteRuleMetadata {
        id: "OPT_I001",
        preconditions: "if-then-else with boolean literal condition",
        negative_cases: "condition is not a literal true/false",
        proof_note: "Dead branch elimination: if true then A else B end = A",
    },
    RewriteRuleMetadata {
        id: "OPT_I002",
        preconditions: "if-then-else with identical then/else branches",
        negative_cases: "branches are different expressions",
        proof_note: "Branch merging: if C then X else X end = X",
    },
    RewriteRuleMetadata {
        id: "OPT_I003",
        preconditions: "nested if with redundant condition check",
        negative_cases: "conditions are not related",
        proof_note: "Condition simplification for nested boolean expressions",
    },
    RewriteRuleMetadata {
        id: "OPT_I004",
        preconditions: "if-then-else with boolean condition and literal branches",
        negative_cases: "branches are not boolean literals",
        proof_note: "Boolean simplification: if C then true else false end = C",
    },
    RewriteRuleMetadata {
        id: "OPT_I005",
        preconditions: "if-then-else with negated condition pattern",
        negative_cases: "branches don't match negation pattern",
        proof_note: "Condition inversion: if C then false else true end = !C",
    },
    RewriteRuleMetadata {
        id: "OPT_B009",
        preconditions: "boolean expression OR true/false",
        negative_cases: "operand is not boolean literal",
        proof_note: "Boolean identity: A or true = true, A or false = A",
    },
    RewriteRuleMetadata {
        id: "OPT_B010",
        preconditions: "boolean expression AND true/false",
        negative_cases: "operand is not boolean literal",
        proof_note: "Boolean absorption: A and true = A, A and false = false",
    },
    RewriteRuleMetadata {
        id: "OPT_P001",
        preconditions: "@len(expr) compared to zero",
        negative_cases: "comparison is not with zero or not @len plugin",
        proof_note: "Length check simplification: @len(x) == 0 = @empty(x)",
    },
    RewriteRuleMetadata {
        id: "OPT_N002",
        preconditions: "negation of comparison operator",
        negative_cases: "inner expression is not a comparison",
        proof_note: "Comparison negation: not (A == B) = A != B",
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

/// Parse if-then-else expression and extract parts
fn parse_if_then_else(expr: &str) -> Option<(&str, &str, &str)> {
    let expr = expr.trim();

    if !expr.starts_with("if ") {
        return None;
    }

    let mut paren_depth = 0;
    let mut if_depth = 0;
    let mut then_pos = None;

    let bytes = expr.as_bytes();
    let mut i = 0;

    while i < bytes.len() - 4 {
        match &bytes[i..i + 1] {
            b"(" => paren_depth += 1,
            b")" => paren_depth -= 1,
            _ => {}
        }

        if paren_depth == 0 && i < bytes.len() - 2 && &bytes[i..i + 3] == b"if " {
            if_depth += 1;
        }

        if paren_depth == 0 && if_depth == 1 && i < bytes.len() - 5 && &bytes[i..i + 6] == b" then "
        {
            then_pos = Some(i);
            break;
        }

        i += 1;
    }

    let then_pos = then_pos?;
    let condition = expr[3..then_pos].trim();

    let rest = &expr[then_pos + 6..];
    let mut else_pos = None;
    let mut nested_if = 0;
    paren_depth = 0;

    let bytes = rest.as_bytes();
    i = 0;

    while i < bytes.len() - 5 {
        match &bytes[i..i + 1] {
            b"(" => paren_depth += 1,
            b")" => paren_depth -= 1,
            _ => {}
        }

        if paren_depth == 0 && i < bytes.len() - 2 && &bytes[i..i + 3] == b"if " {
            nested_if += 1;
        }

        if paren_depth == 0 && i < bytes.len() - 5 && &bytes[i..i + 6] == b" else " {
            if nested_if == 0 {
                else_pos = Some(i);
                break;
            }
            nested_if -= 1;
        }

        i += 1;
    }

    let else_pos = else_pos?;
    let then_expr = rest[..else_pos].trim();

    let else_and_end = &rest[else_pos + 6..];
    let else_expr = else_and_end.strip_suffix(" end")?.trim();

    Some((condition, then_expr, else_expr))
}

/// Dead branch elimination: if true then A else B = A
fn suggest_dead_branch_elimination(expr: &str) -> Option<(&'static str, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if condition == "true" {
        return Some(("OPT_I001", then_expr.to_string()));
    }

    if condition == "false" {
        return Some(("OPT_I001", else_expr.to_string()));
    }

    None
}

/// Branch merging: if C then X else X = X
fn suggest_branch_merging(expr: &str) -> Option<(&'static str, String)> {
    let (_condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == else_expr {
        return Some(("OPT_I002", then_expr.to_string()));
    }

    None
}

/// Nested if simplification: if A then (if A then X else Y) else Z = if A then X else Z
fn suggest_nested_if_simplification(expr: &str) -> Option<(&'static str, String)> {
    let (outer_cond, inner_expr, else_expr) = parse_if_then_else(expr)?;

    // Strip parentheses from inner expression if present
    let inner_stripped = inner_expr.trim();
    let inner_stripped = if inner_stripped.starts_with('(') && inner_stripped.ends_with(')') {
        &inner_stripped[1..inner_stripped.len() - 1]
    } else {
        inner_stripped
    };

    let (inner_cond, inner_then, _inner_else) = parse_if_then_else(inner_stripped)?;

    if outer_cond == inner_cond {
        let result = format!(
            "if {} then {} else {} end",
            outer_cond, inner_then, else_expr
        );
        return Some(("OPT_I003", result));
    }

    None
}

/// Boolean simplification: if C then true else false = C
fn suggest_boolean_simplification(expr: &str) -> Option<(&'static str, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == "true" && else_expr == "false" {
        return Some(("OPT_I004", condition.to_string()));
    }

    None
}

/// Condition inversion: if C then false else true = !C
fn suggest_condition_inversion(expr: &str) -> Option<(&'static str, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == "false" && else_expr == "true" {
        return Some(("OPT_I005", format!("!{}", condition)));
    }

    None
}

/// Boolean identity/absorption: A or true = true, A and false = false
fn suggest_boolean_identity_laws(expr: &str) -> Option<(&'static str, String)> {
    let expr = expr.trim();

    // Check for "or true" / "or false"
    if let Some(or_pos) = expr.find(" or ") {
        let left = expr[..or_pos].trim();
        let right = expr[or_pos + 4..].trim();

        if right == "true" || left == "true" {
            return Some(("OPT_B009", "true".to_string()));
        }
        if right == "false" {
            return Some(("OPT_B009", left.to_string()));
        }
        if left == "false" {
            return Some(("OPT_B009", right.to_string()));
        }
    }

    // Check for "and true" / "and false"
    if let Some(and_pos) = expr.find(" and ") {
        let left = expr[..and_pos].trim();
        let right = expr[and_pos + 5..].trim();

        if left == "true" {
            return Some(("OPT_B010", right.to_string()));
        }
        if right == "true" {
            return Some(("OPT_B010", left.to_string()));
        }
        if left == "false" || right == "false" {
            return Some(("OPT_B010", "false".to_string()));
        }
    }

    None
}

/// Plugin-specific: @len(.x) == 0 → @empty(.x)
fn suggest_plugin_length_simplification(expr: &str) -> Option<(&'static str, String)> {
    let expr = expr.trim();

    // Patterns: @len(.x) == 0, @len(.x) != 0, @len(.x) > 0
    let operators = [(" == ", "=="), (" != ", "!="), (" > ", ">"), (" < ", "<")];

    for (op_str, op_name) in operators {
        if let Some(op_pos) = expr.find(op_str) {
            let left = expr[..op_pos].trim();
            let right = expr[op_pos + op_str.len()..].trim();

            // Check if left side is @len(...) and right side is 0
            if left.starts_with("@len(") && left.ends_with(')') && right == "0" {
                let inner = &left[5..left.len() - 1]; // Extract content inside @len(...)

                if op_name == "==" {
                    return Some(("OPT_P001", format!("@empty({})", inner)));
                }
                if op_name == "!=" || op_name == ">" {
                    return Some(("OPT_P001", format!("@not_empty({})", inner)));
                }
                if op_name == "<" {
                    // @len(.x) < 0 is always false (length can't be negative)
                    return Some(("OPT_P001", "false".to_string()));
                }
            }

            // Check reverse: 0 == @len(.x)
            if right.starts_with("@len(") && right.ends_with(')') && left == "0" {
                let inner = &right[5..right.len() - 1];

                if op_name == "==" {
                    return Some(("OPT_P001", format!("@empty({})", inner)));
                }
                if op_name == "!=" || op_name == ">" {
                    return Some(("OPT_P001", format!("@not_empty({})", inner)));
                }
            }
        }
    }

    None
}

/// Comparison negation: not (.x == 5) → .x != 5
fn suggest_comparison_negation(expr: &str) -> Option<(&'static str, String)> {
    let expr = expr.trim();

    if !expr.starts_with("not (") || !expr.ends_with(')') {
        return None;
    }

    let inner = expr[5..expr.len() - 1].trim();

    // Comparison operators to negate
    let negations = [
        (" == ", " != "),
        (" != ", " == "),
        (" > ", " <= "),
        (" < ", " >= "),
        (" >= ", " < "),
        (" <= ", " > "),
    ];

    for (op, neg_op) in negations {
        if let Some(op_pos) = inner.find(op) {
            let left = inner[..op_pos].trim();
            let right = inner[op_pos + op.len()..].trim();

            if !left.is_empty() && !right.is_empty() {
                return Some(("OPT_N002", format!("{}{}{}", left, neg_op, right)));
            }
        }
    }

    None
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

    if let Some((rule_id, rewrite)) = suggest_reflexive_idempotent_inequality(expr, signatures) {
        return Some((rule_id, rewrite));
    }

    // If-then-else optimizations
    if let Some((rule_id, rewrite)) = suggest_dead_branch_elimination(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_branch_merging(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_nested_if_simplification(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_boolean_simplification(expr) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_condition_inversion(expr) {
        return Some((rule_id, rewrite));
    }

    // Boolean identity/absorption laws
    if let Some((rule_id, rewrite)) = suggest_boolean_identity_laws(expr) {
        return Some((rule_id, rewrite));
    }

    // Plugin-specific optimizations
    if let Some((rule_id, rewrite)) = suggest_plugin_length_simplification(expr) {
        return Some((rule_id, rewrite));
    }

    // Comparison negation normalization
    suggest_comparison_negation(expr)
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
            "OPT_B008", "OPT_B009", "OPT_B010", "OPT_N001", "OPT_N002", "OPT_I001", "OPT_I002",
            "OPT_I003", "OPT_I004", "OPT_I005", "OPT_P001",
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

    // === If-then-else optimization tests ===

    #[test]
    fn test_dead_branch_elimination_true() {
        let (rule_id, rewritten) =
            suggest_dead_branch_elimination("if true then \"yes\" else \"no\" end").unwrap();
        assert_eq!(rule_id, "OPT_I001");
        assert_eq!(rewritten, "\"yes\"");
    }

    #[test]
    fn test_dead_branch_elimination_false() {
        let (rule_id, rewritten) =
            suggest_dead_branch_elimination("if false then \"yes\" else \"no\" end").unwrap();
        assert_eq!(rule_id, "OPT_I001");
        assert_eq!(rewritten, "\"no\"");
    }

    #[test]
    fn test_branch_merging() {
        let (rule_id, rewritten) =
            suggest_branch_merging("if .x > 0 then \"same\" else \"same\" end").unwrap();
        assert_eq!(rule_id, "OPT_I002");
        assert_eq!(rewritten, "\"same\"");
    }

    #[test]
    fn test_nested_if_simplification() {
        // Pattern: if A then (if A then X else Y end) else Z end
        // Simplified: if A then X else Z end
        let input =
            "if .a > 0 then (if .a > 0 then \"inner\" else \"other\" end) else \"outer\" end";
        let result = suggest_nested_if_simplification(input);
        assert!(result.is_some());
        let (rule_id, rewritten) = result.unwrap();
        assert_eq!(rule_id, "OPT_I003");
        assert_eq!(rewritten, "if .a > 0 then \"inner\" else \"outer\" end");
    }

    #[test]
    fn test_parse_if_then_else_simple() {
        let (cond, then_expr, else_expr) =
            parse_if_then_else("if .x > 0 then \"yes\" else \"no\" end").unwrap();
        assert_eq!(cond, ".x > 0");
        assert_eq!(then_expr, "\"yes\"");
        assert_eq!(else_expr, "\"no\"");
    }

    #[test]
    fn test_parse_if_then_else_nested() {
        let (cond, then_expr, else_expr) = parse_if_then_else(
            "if .a > 0 then (if .b > 0 then \"both\" else \"a only\" end) else \"none\" end",
        )
        .unwrap();
        assert_eq!(cond, ".a > 0");
        assert_eq!(then_expr, "(if .b > 0 then \"both\" else \"a only\" end)");
        assert_eq!(else_expr, "\"none\"");
    }

    #[test]
    fn test_collect_optimizations_detects_dead_branch() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if true then "always" else "never" end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_I001");
        assert_eq!(hints[0].after, "\"always\"");
    }

    #[test]
    fn test_collect_optimizations_detects_branch_merging() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if .x > 0 then "same" else "same" end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_I002");
        assert_eq!(hints[0].after, "\"same\"");
    }

    #[test]
    fn test_boolean_simplification() {
        let (rule_id, rewritten) =
            suggest_boolean_simplification("if .x > 0 then true else false end").unwrap();
        assert_eq!(rule_id, "OPT_I004");
        assert_eq!(rewritten, ".x > 0");
    }

    #[test]
    fn test_condition_inversion() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if .x > 0 then false else true end").unwrap();
        assert_eq!(rule_id, "OPT_I005");
        assert_eq!(rewritten, "!.x > 0");
    }

    #[test]
    fn test_collect_optimizations_boolean_simplification() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if @has_header("x") then true else false end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_I004");
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_condition_inversion() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if .status == 200 then false else true end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_I005");
        assert_eq!(hints[0].after, "!.status == 200");
    }

    // === New optimization rules tests ===

    #[test]
    fn test_boolean_identity_or() {
        // A or true = true
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x or true").unwrap();
        assert_eq!(rule_id, "OPT_B009");
        assert_eq!(rewritten, "true");

        // A or false = A
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x or false").unwrap();
        assert_eq!(rule_id, "OPT_B009");
        assert_eq!(rewritten, ".x");

        // true or A = true
        let (rule_id, rewritten) = suggest_boolean_identity_laws("true or .x").unwrap();
        assert_eq!(rule_id, "OPT_B009");
        assert_eq!(rewritten, "true");
    }

    #[test]
    fn test_boolean_absorption_and() {
        // A and true = A
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x and true").unwrap();
        assert_eq!(rule_id, "OPT_B010");
        assert_eq!(rewritten, ".x");

        // A and false = false
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x and false").unwrap();
        assert_eq!(rule_id, "OPT_B010");
        assert_eq!(rewritten, "false");

        // false and A = false
        let (rule_id, rewritten) = suggest_boolean_identity_laws("false and .x").unwrap();
        assert_eq!(rule_id, "OPT_B010");
        assert_eq!(rewritten, "false");
    }

    #[test]
    fn test_plugin_length_simplification() {
        // @len(.x) == 0 → @empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) == 0").unwrap();
        assert_eq!(rule_id, "OPT_P001");
        assert_eq!(rewritten, "@empty(.items)");

        // @len(.x) != 0 → @not_empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) != 0").unwrap();
        assert_eq!(rule_id, "OPT_P001");
        assert_eq!(rewritten, "@not_empty(.items)");

        // @len(.x) > 0 → @not_empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) > 0").unwrap();
        assert_eq!(rule_id, "OPT_P001");
        assert_eq!(rewritten, "@not_empty(.items)");

        // 0 == @len(.x) → @empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("0 == @len(.items)").unwrap();
        assert_eq!(rule_id, "OPT_P001");
        assert_eq!(rewritten, "@empty(.items)");
    }

    #[test]
    fn test_comparison_negation() {
        // not (.x == 5) → .x != 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x == 5)").unwrap();
        assert_eq!(rule_id, "OPT_N002");
        assert_eq!(rewritten, ".x != 5");

        // not (.x != 5) → .x == 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x != 5)").unwrap();
        assert_eq!(rule_id, "OPT_N002");
        assert_eq!(rewritten, ".x == 5");

        // not (.x > 5) → .x <= 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x > 5)").unwrap();
        assert_eq!(rule_id, "OPT_N002");
        assert_eq!(rewritten, ".x <= 5");

        // not (.x >= 5) → .x < 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x >= 5)").unwrap();
        assert_eq!(rule_id, "OPT_N002");
        assert_eq!(rewritten, ".x < 5");
    }

    #[test]
    fn test_collect_optimizations_boolean_identity() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") or true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_B009");
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn test_collect_optimizations_plugin_length() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items) == 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_P001");
        assert_eq!(hints[0].after, "@empty(.items)");
    }

    #[test]
    fn test_collect_optimizations_comparison_negation() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
not (.status == 200)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, "OPT_N002");
        assert_eq!(hints[0].after, ".status != 200");
    }
}
