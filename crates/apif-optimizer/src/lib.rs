use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use apif_parser as parser;
use apif_parser::assertions::strip_assertion_comments;
use apif_plugins::{PluginSignature, TypeInfo, extract_plugin_call_name};
use apif_utils::section_content_line;

fn likely_needs_assertion_rewrite(expr: &str) -> bool {
    expr.contains("==")
        || expr.contains("!=")
        || expr.contains('>')
        || expr.contains('<')
        || expr.contains(" startswith ")
        || expr.contains(" endswith ")
        || expr.contains("!!")
        || expr.contains("not not ")
        || expr.contains("if ")
        || expr.contains(" then ")
        || expr.contains(" else ")
        || expr.contains(" or ")
        || expr.contains(" and ")
        || expr.contains("@len(")
        || expr.contains(">= 0")
        || expr.contains("<= @")
        || expr.starts_with('(')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizationMode {
    #[cfg(test)]
    Conservative,
    AstCanonical,
}

fn normalization_mode() -> NormalizationMode {
    NormalizationMode::AstCanonical
}

fn normalize_expr_for_optimizer_with_mode<'a>(
    expr: &'a str,
    mode: NormalizationMode,
) -> Cow<'a, str> {
    let trimmed = expr.trim();
    match mode {
        #[cfg(test)]
        NormalizationMode::Conservative => Cow::Borrowed(trimmed),
        NormalizationMode::AstCanonical => canonicalize_expr_with_ast(trimmed)
            .map(Cow::Owned)
            .unwrap_or_else(|| Cow::Borrowed(trimmed)),
    }
}

fn canonicalize_expr_with_ast(expr: &str) -> Option<String> {
    use apif_parser::assertion_ast::AssertionExpr;

    fn ast_to_if_string(expr: &AssertionExpr, out: &mut String, prec: u8) {
        match expr {
            AssertionExpr::Or { left, right } => {
                if prec > 1 {
                    out.push('(');
                }
                ast_to_if_string(left, out, 1);
                out.push_str(" or ");
                ast_to_if_string(right, out, 1);
                if prec > 1 {
                    out.push(')');
                }
            }
            AssertionExpr::Xor { left, right } => {
                if prec > 1 {
                    out.push('(');
                }
                ast_to_if_string(left, out, 1);
                out.push_str(" xor ");
                ast_to_if_string(right, out, 1);
                if prec > 1 {
                    out.push(')');
                }
            }
            AssertionExpr::And { left, right } => {
                if prec > 2 {
                    out.push('(');
                }
                ast_to_if_string(left, out, 2);
                out.push_str(" and ");
                ast_to_if_string(right, out, 2);
                if prec > 2 {
                    out.push(')');
                }
            }
            AssertionExpr::Binary { op, left, right } => {
                if prec > 3 {
                    out.push('(');
                }
                ast_to_if_string(left, out, 3);
                out.push(' ');
                out.push_str(op.as_str());
                out.push(' ');
                ast_to_if_string(right, out, 3);
                if prec > 3 {
                    out.push(')');
                }
            }
            AssertionExpr::Not(inner) => {
                out.push('!');
                ast_to_if_string(inner, out, 4);
            }
            AssertionExpr::NotNot(inner) => {
                out.push_str("not not ");
                ast_to_if_string(inner, out, 4);
            }
            AssertionExpr::IfThenElse {
                condition,
                then_branch,
                else_branch,
            } => {
                out.push_str("if ");
                ast_to_if_string(condition, out, 0);
                out.push_str(" then ");
                ast_to_if_string(then_branch, out, 0);
                out.push_str(" else ");
                ast_to_if_string(else_branch, out, 0);
                out.push_str(" end");
            }
            AssertionExpr::Paren(inner) => {
                out.push('(');
                ast_to_if_string(inner, out, 0);
                out.push(')');
            }
            AssertionExpr::Atom(atom) => out.push_str(&atom.to_string()),
            AssertionExpr::Raw(raw) => out.push_str(raw),
        }
    }

    if expr.is_empty() {
        return None;
    }

    let parsed = parser::assertion_ast::parse_assertion(expr);
    let reduced = parser::assertion_ast::remove_redundant_parens(&parsed);
    let mut out = String::with_capacity(expr.len());
    ast_to_if_string(&reduced, &mut out, 0);
    Some(out)
}

#[derive(Debug, Clone, Copy)]
struct RewriteRuleMetadata {
    id: RuleId,
    preconditions: &'static str,
    negative_cases: &'static str,
    proof_note: &'static str,
}

macro_rules! rule_id_table {
    ($($name:ident => $value:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum RuleId {
            $($name),+
        }

        impl RuleId {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$name => $value),+
                }
            }
        }

        impl TryFrom<&str> for RuleId {
            type Error = &'static str;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$name)),+,
                    _ => Err("unknown optimizer rule id"),
                }
            }
        }

        pub mod rule_ids {
            use super::RuleId;
            $(pub const $name: RuleId = RuleId::$name;)+
        }
    };
}

rule_id_table! {
    B001 => "OPT_B001",
    B002 => "OPT_B002",
    B003 => "OPT_B003",
    B004 => "OPT_B004",
    B005 => "OPT_B005",
    B006 => "OPT_B006",
    B007 => "OPT_B007",
    B008 => "OPT_B008",
    B009 => "OPT_B009",
    B010 => "OPT_B010",
    B013 => "OPT_B013",
    B014 => "OPT_B014",
    B015 => "OPT_B015",
    B016 => "OPT_B016",
    B017 => "OPT_B017",
    N001 => "OPT_N001",
    N002 => "OPT_N002",
    I001 => "OPT_I001",
    I002 => "OPT_I002",
    I003 => "OPT_I003",
    I004 => "OPT_I004",
    I005 => "OPT_I005",
    P001 => "OPT_P001",
    P002 => "OPT_P002",
    T001 => "OPT_T001",
    T002 => "OPT_T002",
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for RuleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RuleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        RuleId::try_from(s.as_str()).map_err(serde::de::Error::custom)
    }
}

const REWRITE_RULES: &[RewriteRuleMetadata] = &[
    RewriteRuleMetadata {
        id: rule_ids::B001,
        preconditions: "lhs is boolean plugin expr and rhs is true",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: expr == true is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B002,
        preconditions: "lhs is boolean plugin expr and rhs is false",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: expr == false is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B003,
        preconditions: "lhs is true and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: true == expr is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B004,
        preconditions: "lhs is false and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: false == expr is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B005,
        preconditions: "expression has form !!<bool-plugin-expr>",
        negative_cases: "inner expr is not proven boolean-safe",
        proof_note: "Double negation elimination for boolean expressions",
    },
    RewriteRuleMetadata {
        id: rule_ids::B006,
        preconditions: "binary compare over two literals only",
        negative_cases: "contains non-literals, dynamic plugin calls, or unknown values",
        proof_note: "Constant folding preserves comparison result",
    },
    RewriteRuleMetadata {
        id: rule_ids::B007,
        preconditions: "expression has form x == x and x is idempotent",
        negative_cases: "x may be non-idempotent or side-effectful",
        proof_note: "Reflexive equality over idempotent expressions is always true",
    },
    RewriteRuleMetadata {
        id: rule_ids::B008,
        preconditions: "expression has form x != x and x is idempotent",
        negative_cases: "x may be non-idempotent or side-effectful",
        proof_note: "Reflexive inequality over idempotent expressions is always false",
    },
    RewriteRuleMetadata {
        id: rule_ids::B013,
        preconditions: "lhs is boolean plugin expr and rhs is true",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: expr != true is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B014,
        preconditions: "lhs is boolean plugin expr and rhs is false",
        negative_cases: "lhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: expr != false is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B015,
        preconditions: "lhs is true and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean negation: true != expr is equivalent to !expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B016,
        preconditions: "lhs is false and rhs is boolean plugin expr",
        negative_cases: "rhs is non-boolean, side-effectful, or unsafe-for-rewrite",
        proof_note: "Boolean identity: false != expr is equivalent to expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::B017,
        preconditions: "expression has form not not <bool-plugin-expr>",
        negative_cases: "inner expr is not proven boolean-safe",
        proof_note: "Word-style double negation elimination",
    },
    RewriteRuleMetadata {
        id: rule_ids::N001,
        preconditions: "operator alias startswith/endswith is present",
        negative_cases: "already canonicalized form",
        proof_note: "Canonical spelling rewrite preserves operator semantics",
    },
    RewriteRuleMetadata {
        id: rule_ids::I001,
        preconditions: "if-then-else with boolean literal condition",
        negative_cases: "condition is not a literal true/false",
        proof_note: "Dead branch elimination: if true then A else B end = A",
    },
    RewriteRuleMetadata {
        id: rule_ids::I002,
        preconditions: "if-then-else with identical then/else branches",
        negative_cases: "branches are different expressions",
        proof_note: "Branch merging: if C then X else X end = X",
    },
    RewriteRuleMetadata {
        id: rule_ids::I003,
        preconditions: "nested if with redundant condition check",
        negative_cases: "conditions are not related",
        proof_note: "Condition simplification for nested boolean expressions",
    },
    RewriteRuleMetadata {
        id: rule_ids::I004,
        preconditions: "if-then-else with boolean condition and literal branches",
        negative_cases: "branches are not boolean literals",
        proof_note: "Boolean simplification: if C then true else false end = C",
    },
    RewriteRuleMetadata {
        id: rule_ids::I005,
        preconditions: "if-then-else with negated condition pattern",
        negative_cases: "branches don't match negation pattern",
        proof_note: "Condition inversion: if C then false else true end = !C",
    },
    RewriteRuleMetadata {
        id: rule_ids::B009,
        preconditions: "boolean expression OR true/false",
        negative_cases: "operand is not boolean literal",
        proof_note: "Boolean identity: A or true = true, A or false = A",
    },
    RewriteRuleMetadata {
        id: rule_ids::B010,
        preconditions: "boolean expression AND true/false",
        negative_cases: "operand is not boolean literal",
        proof_note: "Boolean absorption: A and true = A, A and false = false",
    },
    RewriteRuleMetadata {
        id: rule_ids::P001,
        preconditions: "@len(expr) compared to zero",
        negative_cases: "comparison is not with zero or not @len plugin",
        proof_note: "Length check simplification: @len(x) == 0 = @empty(x)",
    },
    RewriteRuleMetadata {
        id: rule_ids::P002,
        preconditions: "expression wrapped in outer parentheses only",
        negative_cases: "inner expression has internal parentheses (ambiguity risk)",
        proof_note: "Redundant parentheses removal: (expr) = expr",
    },
    RewriteRuleMetadata {
        id: rule_ids::N002,
        preconditions: "negation of comparison operator",
        negative_cases: "inner expression is not a comparison",
        proof_note: "Comparison negation: not (A == B) = A != B",
    },
    RewriteRuleMetadata {
        id: rule_ids::T001,
        preconditions: "lhs is UInt plugin expr and rhs is 0",
        negative_cases: "non-zero or non-UInt plugin",
        proof_note: "UInt is always >= 0, so the comparison is always true",
    },
    RewriteRuleMetadata {
        id: rule_ids::T002,
        preconditions: "expression has `:TypeName` suffix and the inner expression already has that type",
        negative_cases: "expression has `:TypeName` but the inner expression has a different or unknown type",
        proof_note: "Type annotation is redundant when the type is already known",
    },
];

fn rule_metadata(rule_id: RuleId) -> Option<&'static RewriteRuleMetadata> {
    REWRITE_RULES.iter().find(|r| r.id == rule_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationHint {
    pub rule_id: RuleId,
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

fn build_hint(rule_id: RuleId, line: usize, before: &str, after: String) -> OptimizationHint {
    let meta = rule_metadata(rule_id);
    OptimizationHint {
        rule_id,
        line,
        before: before.to_string(),
        after,
        preconditions: meta.map(|m| m.preconditions.to_string()),
        negative_cases: meta.map(|m| m.negative_cases.to_string()),
        proof_note: meta.map(|m| m.proof_note.to_string()),
    }
}

use apif_plugins::PLUGIN_SIGNATURES;

static BOOLEAN_PLUGINS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    PLUGIN_SIGNATURES
        .iter()
        .filter(|(_, signature)| {
            signature.return_type == TypeInfo::Bool
                && signature.safe_for_rewrite
                && signature.deterministic
                && signature.idempotent
        })
        .map(|(name, _)| name.clone())
        .collect()
});

fn plugin_signatures() -> &'static HashMap<String, PluginSignature> {
    &PLUGIN_SIGNATURES
}

fn boolean_plugins() -> &'static HashSet<String> {
    &BOOLEAN_PLUGINS
}

fn is_boolean_plugin_expr(expr: &str, bool_plugins: &HashSet<String>) -> bool {
    let Some(plugin_name) = extract_plugin_call_name(expr) else {
        return false;
    };

    bool_plugins.contains(plugin_name.as_str())
}

fn suggest_boolean_rewrite(expr: &str, bool_plugins: &HashSet<String>) -> Option<(RuleId, String)> {
    let (lhs, rhs) = expr.split_once("==")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "true" {
        return Some((rule_ids::B001, lhs.to_string()));
    }
    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "false" {
        return Some((rule_ids::B002, format!("!{}", lhs)));
    }
    if lhs == "true" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some((rule_ids::B003, rhs.to_string()));
    }
    if lhs == "false" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some((rule_ids::B004, format!("!{}", rhs)));
    }

    None
}

fn suggest_not_not_rewrite(expr: &str, bool_plugins: &HashSet<String>) -> Option<(RuleId, String)> {
    let trimmed = expr.trim();
    if !trimmed.starts_with("not not ") {
        return None;
    }

    let inner = trimmed[8..].trim();
    if is_boolean_plugin_expr(inner, bool_plugins) {
        return Some((rule_ids::B017, inner.to_string()));
    }

    None
}

fn suggest_inequality_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
) -> Option<(RuleId, String)> {
    let (lhs, rhs) = expr.split_once("!=")?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "true" {
        return Some((rule_ids::B013, format!("!{}", lhs)));
    }
    if is_boolean_plugin_expr(lhs, bool_plugins) && rhs == "false" {
        return Some((rule_ids::B014, lhs.to_string()));
    }
    if lhs == "true" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some((rule_ids::B015, format!("!{}", rhs)));
    }
    if lhs == "false" && is_boolean_plugin_expr(rhs, bool_plugins) {
        return Some((rule_ids::B016, rhs.to_string()));
    }

    None
}

/// Redundant parentheses: (expr) -> expr (single expression, no ambiguity)
fn suggest_redundant_parens(expr: &str) -> Option<(RuleId, String)> {
    let trimmed = expr.trim();
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return None;
    }

    let inner = &trimmed[1..trimmed.len() - 1].trim();
    if inner.is_empty() {
        return None;
    }

    let balanced = inner.chars().fold(0i32, |acc, c| {
        if c == '(' {
            acc + 1
        } else if c == ')' {
            acc - 1
        } else {
            acc
        }
    });
    if balanced != 0 {
        return None;
    }

    Some((rule_ids::P002, inner.to_string()))
}

fn suggest_double_negation_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
) -> Option<(RuleId, String)> {
    let trimmed = expr.trim();
    if !trimmed.starts_with("!!") {
        return None;
    }

    let inner = trimmed[2..].trim();
    if is_boolean_plugin_expr(inner, bool_plugins) {
        return Some((rule_ids::B005, inner.to_string()));
    }

    None
}

fn suggest_operator_canonicalization(expr: &str) -> Option<(RuleId, String)> {
    if expr.contains(" startswith ") {
        let rewritten = expr.replace(" startswith ", " startsWith ");
        return Some((rule_ids::N001, rewritten));
    }
    if expr.contains(" endswith ") {
        let rewritten = expr.replace(" endswith ", " endsWith ");
        return Some((rule_ids::N001, rewritten));
    }
    None
}

fn parse_literal(expr: &str) -> Option<serde_json::Value> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "true" {
        return Some(serde_json::Value::Bool(true));
    }
    if trimmed == "false" {
        return Some(serde_json::Value::Bool(false));
    }
    if trimmed == "null" {
        return Some(serde_json::Value::Null);
    }

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return serde_json::from_str(trimmed).ok();
    }

    if let Ok(i) = trimmed.parse::<i64>() {
        return Some(serde_json::Value::Number(serde_json::Number::from(i)));
    }

    if let Ok(f) = trimmed.parse::<f64>() {
        return serde_json::Number::from_f64(f).map(serde_json::Value::Number);
    }

    None
}

fn suggest_constant_folding(expr: &str) -> Option<(RuleId, String)> {
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
            ">" | "<" | ">=" | "<=" => compare_literal_numbers(&lhs, &rhs, op),
            _ => None,
        }?;

        return Some((rule_ids::B006, folded.to_string()));
    }

    None
}

fn compare_literal_numbers(
    lhs: &serde_json::Value,
    rhs: &serde_json::Value,
    op: &str,
) -> Option<bool> {
    let lhs_num = lhs.as_number()?;
    let rhs_num = rhs.as_number()?;

    let lhs_i = lhs_num
        .as_i64()
        .map(i128::from)
        .or_else(|| lhs_num.as_u64().map(i128::from));
    let rhs_i = rhs_num
        .as_i64()
        .map(i128::from)
        .or_else(|| rhs_num.as_u64().map(i128::from));

    if let (Some(l), Some(r)) = (lhs_i, rhs_i) {
        return Some(match op {
            ">" => l > r,
            "<" => l < r,
            ">=" => l >= r,
            "<=" => l <= r,
            _ => unreachable!(),
        });
    }

    let (l, r) = (lhs_num.as_f64()?, rhs_num.as_f64()?);
    Some(match op {
        ">" => l > r,
        "<" => l < r,
        ">=" => l >= r,
        "<=" => l <= r,
        _ => unreachable!(),
    })
}

fn is_idempotent_expr(expr: &str, signatures: &HashMap<String, PluginSignature>) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }

    if parse_literal(trimmed).is_some() {
        return true;
    }

    if (trimmed.starts_with("{{") && trimmed.ends_with("}}"))
        || trimmed.starts_with('$')
        || trimmed.starts_with('.')
    {
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

fn suggest_reflexive_idempotent(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<(RuleId, String)> {
    let (_op, lhs, rhs, rule_id, result) = if let Some((l, r)) = expr.split_once("==") {
        ("==", l, r, rule_ids::B007, "true")
    } else if let Some((l, r)) = expr.split_once("!=") {
        ("!=", l, r, rule_ids::B008, "false")
    } else {
        return None;
    };

    let lhs = lhs.trim();
    let rhs = rhs.trim();

    if lhs.is_empty() || rhs.is_empty() || lhs != rhs {
        return None;
    }

    if parse_literal(lhs).is_some() && parse_literal(rhs).is_some() {
        return None;
    }

    if !is_idempotent_expr(lhs, signatures) {
        return None;
    }

    Some((rule_id, result.to_string()))
}

/// Parse if-then-else expression and extract parts
fn parse_if_then_else(expr: &str) -> Option<(&str, &str, &str)> {
    let expr = expr.trim();

    if !expr.starts_with("if ") {
        return None;
    }

    let bytes = expr.as_bytes();
    let mut paren_depth = 0;
    let mut if_depth = 0;
    let mut then_pos = None;

    let mut i = 0;
    let mut in_string = false;
    let mut string_char = None;
    while i < bytes.len() {
        // Handle string literals
        if in_string {
            if let Some(quote) = string_char
                && bytes[i] == quote
                && (i == 0 || bytes[i - 1] != b'\\')
            {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            in_string = true;
            string_char = Some(bytes[i]);
            i += 1;
            continue;
        }

        match &bytes[i..i + 1] {
            b"(" => paren_depth += 1,
            b")" => paren_depth -= 1,
            _ => {}
        }

        if paren_depth == 0 && i + 3 <= bytes.len() && &bytes[i..i + 3] == b"if " {
            if_depth += 1;
        }

        if paren_depth == 0
            && if_depth == 1
            && i + 6 <= bytes.len()
            && &bytes[i..i + 6] == b" then "
        {
            then_pos = Some(i);
            break;
        }

        i += 1;
    }

    let then_pos = then_pos?;
    let condition = expr[3..then_pos].trim();

    let rest = &expr[then_pos + 6..];
    let bytes = rest.as_bytes();
    let mut else_pos = None;
    let mut nested_if = 0;
    paren_depth = 0;

    let mut in_string = false;
    let mut string_char = None;

    i = 0;
    while i < bytes.len() {
        if in_string {
            if let Some(quote) = string_char
                && bytes[i] == quote
                && (i == 0 || bytes[i - 1] != b'\\')
            {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            in_string = true;
            string_char = Some(bytes[i]);
            i += 1;
            continue;
        }

        match &bytes[i..i + 1] {
            b"(" => paren_depth += 1,
            b")" => paren_depth -= 1,
            _ => {}
        }

        if paren_depth == 0 && i + 3 <= bytes.len() && &bytes[i..i + 3] == b"if " {
            nested_if += 1;
        }

        if paren_depth == 0 && i + 6 <= bytes.len() && &bytes[i..i + 6] == b" else " {
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
fn suggest_dead_branch_elimination(expr: &str) -> Option<(RuleId, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if condition == "true" {
        return Some((rule_ids::I001, then_expr.to_string()));
    }

    if condition == "false" {
        return Some((rule_ids::I001, else_expr.to_string()));
    }

    None
}

/// Branch merging: if C then X else X = X
fn suggest_branch_merging(expr: &str) -> Option<(RuleId, String)> {
    let (_condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == else_expr {
        return Some((rule_ids::I002, then_expr.to_string()));
    }

    None
}

/// Nested if simplification: if A then (if A then X else Y) else Z = if A then X else Z
fn suggest_nested_if_simplification(expr: &str) -> Option<(RuleId, String)> {
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
        return Some((rule_ids::I003, result));
    }

    None
}

/// Boolean simplification: if C then true else false = C
fn suggest_boolean_simplification(expr: &str) -> Option<(RuleId, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == "true" && else_expr == "false" {
        return Some((rule_ids::I004, condition.to_string()));
    }

    None
}

fn needs_parens_for_prefix_not(expr: &str) -> bool {
    use apif_parser::assertion_ast::AssertionExpr;

    let parsed = parser::assertion_ast::parse_assertion(expr.trim());
    let reduced = parser::assertion_ast::remove_redundant_parens(&parsed);

    !matches!(reduced, AssertionExpr::Atom(_))
}

fn negate_condition_expr(condition: &str) -> String {
    if let Some(negated) = negate_comparison_expr(condition) {
        return negated;
    }

    let c = condition.trim();
    if c.starts_with('(') && c.ends_with(')') {
        return format!("!{}", c);
    }

    if needs_parens_for_prefix_not(c) {
        format!("!({})", c)
    } else {
        format!("!{}", c)
    }
}

/// Condition inversion: if C then false else true = !(C)
fn suggest_condition_inversion(expr: &str) -> Option<(RuleId, String)> {
    let (condition, then_expr, else_expr) = parse_if_then_else(expr)?;

    if then_expr == "false" && else_expr == "true" {
        Some((rule_ids::I005, negate_condition_expr(condition)))
    } else {
        None
    }
}

/// Boolean identity/absorption: A or true = true, A and false = false
fn suggest_boolean_identity_laws(expr: &str) -> Option<(RuleId, String)> {
    let expr = expr.trim();

    // Check for "or true" / "or false"
    if let Some(or_pos) = expr.find(" or ") {
        let left = expr[..or_pos].trim();
        let right = expr[or_pos + 4..].trim();

        if right == "true" || left == "true" {
            return Some((rule_ids::B009, "true".to_string()));
        }
        if right == "false" {
            return Some((rule_ids::B009, left.to_string()));
        }
        if left == "false" {
            return Some((rule_ids::B009, right.to_string()));
        }
    }

    // Check for "and true" / "and false"
    if let Some(and_pos) = expr.find(" and ") {
        let left = expr[..and_pos].trim();
        let right = expr[and_pos + 5..].trim();

        if left == "true" {
            return Some((rule_ids::B010, right.to_string()));
        }
        if right == "true" {
            return Some((rule_ids::B010, left.to_string()));
        }
        if left == "false" || right == "false" {
            return Some((rule_ids::B010, "false".to_string()));
        }
    }

    None
}

/// Plugin-specific: @len(.x) == 0 → @empty(.x)
fn suggest_plugin_length_simplification(expr: &str) -> Option<(RuleId, String)> {
    fn extract_len_inner(s: &str) -> Option<&str> {
        if s.starts_with("@len(") && s.ends_with(')') {
            Some(&s[5..s.len() - 1])
        } else {
            None
        }
    }

    fn rewrite_len_zero_cmp(op: &str, inner: &str, len_on_left: bool) -> Option<String> {
        match (op, len_on_left) {
            ("==", _) | ("<=", _) => Some(format!("@empty({})", inner)),
            ("!=", _) => Some(format!("@len({}) > 0", inner)),
            (">", true) => None,
            (">", false) => Some("false".to_string()),
            ("<", true) => Some("false".to_string()),
            ("<", false) => None,
            _ => None,
        }
    }

    let expr = expr.trim();

    // Patterns: @len(.x) == 0, @len(.x) != 0, @len(.x) > 0
    let operators = [
        (" == ", "=="),
        (" != ", "!="),
        (" > ", ">"),
        (" < ", "<"),
        (" <= ", "<="),
    ];

    for (op_str, op_name) in operators {
        if let Some(op_pos) = expr.find(op_str) {
            let left = expr[..op_pos].trim();
            let right = expr[op_pos + op_str.len()..].trim();

            if right == "0"
                && let Some(inner) = extract_len_inner(left)
            {
                return rewrite_len_zero_cmp(op_name, inner, true)
                    .map(|rewrite| (rule_ids::P001, rewrite));
            }

            if left == "0"
                && let Some(inner) = extract_len_inner(right)
            {
                return rewrite_len_zero_cmp(op_name, inner, false)
                    .map(|rewrite| (rule_ids::P001, rewrite));
            }
        }
    }

    None
}

/// Type-aware numeric comparison optimization.
/// Uses TypeInfo to detect that certain plugins return unsigned integers,
/// making comparisons like `@len(.x) >= 0` always true.
fn suggest_type_aware_numeric_comparison(expr: &str) -> Option<(RuleId, String)> {
    let signatures = plugin_signatures();
    let trimmed = expr.trim();

    let (left, right) = if let Some(idx) = trimmed.find(">=") {
        (trimmed[..idx].trim(), trimmed[idx + 2..].trim())
    } else {
        let idx = trimmed.find("<=")?;
        (trimmed[..idx].trim(), trimmed[idx + 2..].trim())
    };

    let plugin_call = if right == "0" {
        left
    } else if left == "0" {
        right
    } else {
        return None;
    };

    if let Some(plugin_name) = extract_plugin_call_name(plugin_call)
        && let Some(sig) = signatures.get(plugin_name.as_str())
        && sig.return_type == TypeInfo::UInt
    {
        Some((rule_ids::T001, "true".to_string()))
    } else {
        None
    }
}

/// Comparison negation: not (.x == 5) → .x != 5
fn suggest_comparison_negation(expr: &str) -> Option<(RuleId, String)> {
    let expr = expr.trim();

    let inner = if expr.starts_with("not (") && expr.ends_with(')') {
        expr[5..expr.len() - 1].trim()
    } else if expr.starts_with("!(") && expr.ends_with(')') {
        expr[2..expr.len() - 1].trim()
    } else {
        return None;
    };

    negate_comparison_expr(inner).map(|rewritten| (rule_ids::N002, rewritten))
}

fn negate_comparison_expr(inner: &str) -> Option<String> {
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
                return Some(format!("{}{}{}", left, neg_op, right));
            }
        }
    }

    None
}

/// Detect redundant type annotations: `@len(.x):uint` → `@len(.x)` when `@len` already returns uint.
fn suggest_redundant_type_cast(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
) -> Option<(RuleId, String)> {
    let colon_pos = expr.rfind(':')?;
    if colon_pos == 0 {
        return None;
    }

    let cast_type_name = &expr[colon_pos + 1..];
    let inner_expr = expr[..colon_pos].trim();

    // Extract the type name (stop at non-alphanumeric chars)
    let cast_type_end = cast_type_name
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(cast_type_name.len());
    let cast_type_name = &cast_type_name[..cast_type_end];
    if cast_type_name.is_empty() {
        return None;
    }

    // Only consider casts into valid TypeInfo names
    let cast_type = TypeInfo::parse_type_name(cast_type_name)?;

    // Infer type of inner expression
        let inner_tokens = parser::tokenizer::tokenize_assertion(inner_expr);
        let empty_vars = std::collections::HashMap::new();
        let inner_type = apif_semantics::infer_type_from_tokens(
            &inner_tokens, signatures, &empty_vars,
        );

    // If inner type is unknown, cast might be useful — don't flag
    if inner_type == TypeInfo::Any || inner_type == TypeInfo::Yaml || inner_type == TypeInfo::Json {
        return None;
    }

    // If the cast type matches the inferred type, it's redundant
    let cast_base = cast_type.base_type();
    let inner_base = inner_type.base_type();

    let types_match =
        cast_base == inner_base || (cast_base.is_numeric() && inner_base.is_numeric());

    if !types_match {
        return None;
    }

    // Build the rewritten expression by removing the `:type` suffix
    let after_colon = &expr[colon_pos + 1..];
    let rest = after_colon[cast_type_name.len()..].trim();

    let rewritten = if rest.is_empty() {
        inner_expr.to_string()
    } else {
        format!("{} {}", inner_expr, rest)
    };

    Some((rule_ids::T002, rewritten))
}

fn rewrite_assertion_expression_with_context(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    bool_plugins: &HashSet<String>,
    normalization_mode: NormalizationMode,
) -> Option<(RuleId, String)> {
    let normalized = normalize_expr_for_optimizer_with_mode(expr, normalization_mode);
    let expr = normalized.as_ref();

    if let Some((rule_id, rewrite)) = suggest_boolean_rewrite(expr, bool_plugins) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_not_not_rewrite(expr, bool_plugins) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_inequality_rewrite(expr, bool_plugins) {
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

    if let Some((rule_id, rewrite)) = suggest_reflexive_idempotent(expr, signatures) {
        return Some((rule_id, rewrite));
    }

    // Redundant parentheses removal
    if let Some((rule_id, rewrite)) = suggest_redundant_parens(expr) {
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

    // Type-aware optimizations based on TypeInfo
    if let Some((rule_id, rewrite)) = suggest_type_aware_numeric_comparison(expr) {
        return Some((rule_id, rewrite));
    }

    // Redundant type annotation removal
    if let Some((rule_id, rewrite)) = suggest_redundant_type_cast(expr, signatures) {
        return Some((rule_id, rewrite));
    }

    // Comparison negation normalization
    suggest_comparison_negation(expr)
}

fn rewrite_assertion_expression_fixed_point_with_mode(
    expr: &str,
    mode: NormalizationMode,
) -> String {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();

    let mut current = Cow::Borrowed(expr.trim());
    for _ in 0..32 {
        let Some((_, rewritten)) =
            rewrite_assertion_expression_with_context(&current, signatures, bool_plugins, mode)
        else {
            break;
        };

        let normalized = rewritten.trim();
        if normalized == current.as_ref() {
            break;
        }
        current = Cow::Owned(normalized.to_string());
    }

    current.into_owned()
}

pub fn rewrite_assertion_expression(expr: &str) -> Option<(&'static str, String)> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    rewrite_assertion_expression_with_context(expr, signatures, bool_plugins, normalization_mode())
        .map(|(rule_id, rewrite)| (rule_id.as_str(), rewrite))
}

pub fn rewrite_assertion_expression_fixed_point(expr: &str) -> String {
    rewrite_assertion_expression_fixed_point_with_mode(expr, normalization_mode())
}

pub fn rewrite_assertion_expression_fixed_point_if_changed(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() || !likely_needs_assertion_rewrite(trimmed) {
        None
    } else {
        let rewritten = rewrite_assertion_expression_fixed_point(trimmed);
        if rewritten == trimmed {
            None
        } else {
            Some(rewritten)
        }
    }
}

pub fn collect_assertion_optimizations(doc: &parser::GctfDocument) -> Vec<OptimizationHint> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    let mode = normalization_mode();
    let mut hints = Vec::new();

    for section in &doc.sections {
        if section.section_type != parser::ast::SectionType::Asserts {
            continue;
        }

        for (idx, line) in section.raw_content.lines().enumerate() {
            let Some(trimmed) = strip_assertion_comments(line) else {
                continue;
            };

            if !likely_needs_assertion_rewrite(&trimmed) {
                continue;
            }

            if let Some((rule_id, rewrite)) =
                rewrite_assertion_expression_with_context(&trimmed, signatures, bool_plugins, mode)
            {
                debug_assert!(rule_metadata(rule_id).is_some());
                hints.push(build_hint(
                    rule_id,
                    section_content_line(section.start_line, idx),
                    &trimmed,
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

    fn ast_mode_active_for_tests() -> bool {
        matches!(normalization_mode(), NormalizationMode::AstCanonical)
    }

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
        assert_eq!(hints[0].rule_id, rule_ids::B001);
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
        if ast_mode_active_for_tests() {
            assert_eq!(hints[0].rule_id, rule_ids::B017);
        } else {
            assert_eq!(hints[0].rule_id, rule_ids::B005);
        }
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
        if ast_mode_active_for_tests() {
            assert!(hints.is_empty());
        } else {
            assert_eq!(hints.len(), 1);
            assert_eq!(hints[0].rule_id, rule_ids::N001);
            assert_eq!(hints[0].after, ".name startsWith \"abc\"");
        }
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
        assert_eq!(hints[0].rule_id, rule_ids::B006);
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
        assert_eq!(hints[0].rule_id, rule_ids::B006);
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn test_rewrite_rule_metadata_is_complete() {
        let expected = [
            rule_ids::B001,
            rule_ids::B002,
            rule_ids::B003,
            rule_ids::B004,
            rule_ids::B005,
            rule_ids::B006,
            rule_ids::B007,
            rule_ids::B008,
            rule_ids::B009,
            rule_ids::B010,
            rule_ids::B013,
            rule_ids::B014,
            rule_ids::B015,
            rule_ids::B016,
            rule_ids::B017,
            rule_ids::N001,
            rule_ids::N002,
            rule_ids::I001,
            rule_ids::I002,
            rule_ids::I003,
            rule_ids::I004,
            rule_ids::I005,
            rule_ids::P001,
            rule_ids::P002,
            rule_ids::T001,
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
        assert_eq!(hints[0].rule_id, rule_ids::B007);
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
$user_id != $user_id
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B008);
        assert_eq!(hints[0].after, "false");
    }

    #[test]
    fn test_rewrite_assertion_expression_fixed_point() {
        let expr = "true == @has_header(\"x-request-id\")";
        let rewritten = rewrite_assertion_expression_fixed_point(expr);
        assert_eq!(rewritten, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn test_rewrite_assertion_expression_fixed_point_if_changed() {
        assert_eq!(
            rewrite_assertion_expression_fixed_point_if_changed(
                "true == @has_header(\"x-request-id\")"
            ),
            Some("@has_header(\"x-request-id\")".to_string())
        );
        assert_eq!(
            rewrite_assertion_expression_fixed_point_if_changed(".status == 200"),
            None
        );
    }

    #[test]
    fn test_collect_assertion_optimizations_ignores_inline_comments() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
true == @has_header("x-request-id") // comment should be ignored
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B003);
        assert_eq!(hints[0].after, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn test_likely_needs_assertion_rewrite_fast_path() {
        assert!(!likely_needs_assertion_rewrite("@scope_message_count()"));
        assert!(likely_needs_assertion_rewrite("@elapsed_ms() >= 10"));
        assert!(likely_needs_assertion_rewrite("true == @has_header(\"x\")"));
        assert!(likely_needs_assertion_rewrite(".name startswith \"abc\""));
        assert!(likely_needs_assertion_rewrite("if true then 1 else 2 end"));
    }

    // === If-then-else optimization tests ===

    #[test]
    fn test_dead_branch_elimination_true() {
        let (rule_id, rewritten) =
            suggest_dead_branch_elimination("if true then \"yes\" else \"no\" end").unwrap();
        assert_eq!(rule_id, rule_ids::I001);
        assert_eq!(rewritten, "\"yes\"");
    }

    #[test]
    fn test_dead_branch_elimination_false() {
        let (rule_id, rewritten) =
            suggest_dead_branch_elimination("if false then \"yes\" else \"no\" end").unwrap();
        assert_eq!(rule_id, rule_ids::I001);
        assert_eq!(rewritten, "\"no\"");
    }

    #[test]
    fn test_branch_merging() {
        let (rule_id, rewritten) =
            suggest_branch_merging("if .x > 0 then \"same\" else \"same\" end").unwrap();
        assert_eq!(rule_id, rule_ids::I002);
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
        assert_eq!(rule_id, rule_ids::I003);
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
        assert_eq!(hints[0].rule_id, rule_ids::I001);
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
        assert_eq!(hints[0].rule_id, rule_ids::I002);
        assert_eq!(hints[0].after, "\"same\"");
    }

    #[test]
    fn test_boolean_simplification() {
        let (rule_id, rewritten) =
            suggest_boolean_simplification("if .x > 0 then true else false end").unwrap();
        assert_eq!(rule_id, rule_ids::I004);
        assert_eq!(rewritten, ".x > 0");
    }

    #[test]
    fn test_condition_inversion() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if .x > 0 then false else true end").unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, ".x <= 0");
    }

    #[test]
    fn test_condition_inversion_contains_needs_parens() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if .name contains \"foo\" then false else true end")
                .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(.name contains \"foo\")");
    }

    #[test]
    fn test_condition_inversion_simple_plugin_call_no_parens() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if @has_header(\"x\") then false else true end").unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!@has_header(\"x\")");
    }

    #[test]
    fn test_condition_inversion_not_keyword_gets_grouped() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if not @has_header(\"x\") then false else true end")
                .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(not @has_header(\"x\"))");
    }

    #[test]
    fn test_condition_inversion_bang_gets_grouped() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if !@has_header(\"x\") then false else true end").unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(!@has_header(\"x\"))");
    }

    #[test]
    fn test_condition_inversion_matches_gets_grouped() {
        let (rule_id, rewritten) =
            suggest_condition_inversion("if .name matches /foo.*/ then false else true end")
                .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(.name matches /foo.*/)");
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
        assert_eq!(hints[0].rule_id, rule_ids::I004);
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
        assert_eq!(hints[0].rule_id, rule_ids::I005);
        assert_eq!(hints[0].after, ".status != 200");
    }

    #[test]
    fn test_parse_if_then_else_string_with_else_keyword() {
        let (cond, then_expr, else_expr) =
            parse_if_then_else(r#"if true then " else " else "no" end"#).unwrap();
        assert_eq!(cond, "true");
        assert_eq!(then_expr, r#"" else ""#);
        assert_eq!(else_expr, r#""no""#);
    }

    #[test]
    fn test_parse_if_then_else_then_in_string_condition() {
        let (cond, then_expr, else_expr) =
            parse_if_then_else(r#"if .x == "then" then "yes" else "no" end"#).unwrap();
        assert_eq!(cond, r#".x == "then""#);
        assert_eq!(then_expr, r#""yes""#);
        assert_eq!(else_expr, r#""no""#);
    }

    // === New optimization rules tests ===

    #[test]
    fn test_boolean_identity_or() {
        // A or true = true
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x or true").unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, "true");

        // A or false = A
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x or false").unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, ".x");

        // true or A = true
        let (rule_id, rewritten) = suggest_boolean_identity_laws("true or .x").unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, "true");
    }

    #[test]
    fn test_boolean_absorption_and() {
        // A and true = A
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x and true").unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, ".x");

        // A and false = false
        let (rule_id, rewritten) = suggest_boolean_identity_laws(".x and false").unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, "false");

        // false and A = false
        let (rule_id, rewritten) = suggest_boolean_identity_laws("false and .x").unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, "false");
    }

    #[test]
    fn test_plugin_length_simplification() {
        // @len(.x) == 0 → @empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) == 0").unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@empty(.items)");

        // @len(.x) != 0 → @len(.x) > 0
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) != 0").unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@len(.items) > 0");

        // @len(.x) > 0 → no simplification
        let result = suggest_plugin_length_simplification("@len(.items) > 0");
        assert!(result.is_none());

        // 0 == @len(.x) → @empty(.x)
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("0 == @len(.items)").unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@empty(.items)");
    }

    #[test]
    fn test_comparison_negation() {
        // not (.x == 5) → .x != 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x == 5)").unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x != 5");

        // not (.x != 5) → .x == 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x != 5)").unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x == 5");

        // not (.x > 5) → .x <= 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x > 5)").unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x <= 5");

        // not (.x >= 5) → .x < 5
        let (rule_id, rewritten) = suggest_comparison_negation("not (.x >= 5)").unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x < 5");

        // !(.x <= 5) -> .x > 5
        let (rule_id, rewritten) = suggest_comparison_negation("!(.x <= 5)").unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x > 5");

        // malformed/non-comparison inner should not rewrite
        assert!(suggest_comparison_negation("!(.x)").is_none());
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
        assert_eq!(hints[0].rule_id, rule_ids::B009);
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
        assert_eq!(hints[0].rule_id, rule_ids::P001);
        assert_eq!(hints[0].after, "@empty(.items)");
    }

    #[test]
    fn test_collect_optimizations_type_aware_uint_gte_zero() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items) >= 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::T001);
        assert_eq!(hints[0].after, "true");
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
        assert_eq!(hints[0].rule_id, rule_ids::N002);
        assert_eq!(hints[0].after, ".status != 200");
    }

    #[test]
    fn test_collect_optimizations_b002_expr_equals_false() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == false
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B002);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b004_false_equals_expr() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
false == @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B004);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b013_inequality_true() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") != true
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B013);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b014_inequality_false() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") != false
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B014);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b015_true_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
true != @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B015);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b016_false_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
false != @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B016);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_b017_double_not_word() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
not not @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B017);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn test_collect_optimizations_p002_redundant_parens() {
        let result = rewrite_assertion_expression_fixed_point("(@has_header(\"x\"))");
        if ast_mode_active_for_tests() {
            assert_eq!(result, "(@has_header(\"x\"))");
        } else {
            assert_eq!(result, "@has_header(\"x\")");
        }
    }

    #[test]
    fn test_boolean_plugins_contains_uuid() {
        let bp = boolean_plugins();
        assert!(bp.contains("uuid"));
        assert!(bp.contains("email"));
        assert!(bp.contains("empty"));
    }

    #[test]
    fn test_plugin_signatures_returns_map() {
        let sigs = plugin_signatures();
        assert!(!sigs.is_empty());
        assert!(sigs.contains_key("uuid"));
    }

    #[test]
    fn test_is_boolean_plugin_expr() {
        let bp = boolean_plugins();
        assert!(is_boolean_plugin_expr("@uuid(.x)", bp));
        assert!(is_boolean_plugin_expr("@empty(.items)", bp));
        assert!(!is_boolean_plugin_expr("@len(.x)", bp));
    }

    #[test]
    fn test_suggest_constant_folding_string_equality() {
        let result = suggest_constant_folding("\"foo\" == \"foo\"");
        assert!(result.is_some());
        let (rule_id, after) = result.unwrap();
        assert_eq!(rule_id, rule_ids::B006);
        assert_eq!(after, "true");
    }

    #[test]
    fn test_suggest_constant_folding_mixed_types() {
        let result = suggest_constant_folding("\"foo\" == 123");
        assert!(result.is_some());
        let (_rule_id, after) = result.unwrap();
        assert_eq!(after, "false");
    }

    #[test]
    fn test_suggest_constant_folding_invalid_json() {
        let result = suggest_constant_folding("@len(.x) == 5");
        assert!(result.is_none());
    }

    #[test]
    fn test_normalization_mode_is_ast_canonical() {
        assert_eq!(normalization_mode(), NormalizationMode::AstCanonical);
    }

    #[test]
    fn test_ast_mode_can_change_first_matching_rule() {
        let signatures = plugin_signatures();
        let bool_plugins = boolean_plugins();
        let expr = "((@has_header(\"x\"))) == true";

        let conservative = rewrite_assertion_expression_with_context(
            expr,
            signatures,
            bool_plugins,
            NormalizationMode::Conservative,
        );
        let ast = rewrite_assertion_expression_with_context(
            expr,
            signatures,
            bool_plugins,
            NormalizationMode::AstCanonical,
        );

        assert_eq!(conservative.map(|(id, _)| id), None);
        assert_eq!(ast.map(|(id, _)| id), Some(rule_ids::B001));
    }

    #[test]
    fn test_ast_canonical_mode_preserves_execution_result() {
        use apif_assert::engine::{AssertionEngine, AssertionResult};
        use serde_json::json;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Outcome {
            Pass,
            Fail,
            Error,
        }

        fn outcome_of(result: &AssertionResult) -> Outcome {
            match result {
                AssertionResult::Pass => Outcome::Pass,
                AssertionResult::Fail { .. } => Outcome::Fail,
                AssertionResult::Error(_) => Outcome::Error,
            }
        }

        let engine = AssertionEngine::with_registry(std::sync::Arc::new(
            apif_plugins::PluginManager::new(),
        ));
        let cases = [
            "!!@has_header(\"x\")",
            "not not @has_header(\"x\")",
            "@has_header(\"x\") == true",
            "@has_header(\"x\") == false",
            "true != @has_header(\"x\")",
            ".name startswith \"abc\"",
            "not (.status == 200)",
            "if @has_header(\"x\") then true else false end",
            "if .status == 200 then false else true end",
            "if true then \"always\" else \"never\" end",
            "if .x > 0 then \"same\" else \"same\" end",
            "(@has_header(\"x\"))",
            "@len(.items) >= 0",
            "@len(.items) == 0",
            "@has_header(\"x\") == true and .status == 200",
            "true or @has_header(\"x\")",
        ];

        let contexts = vec![
            (
                "status_200_with_header",
                json!({ "status": 200, "name": "abc-xyz", "x": 1, "items": [1, 2] }),
                Some(std::collections::HashMap::from([(
                    "x".to_string(),
                    "1".to_string(),
                )])),
            ),
            (
                "status_200_without_header",
                json!({ "status": 200, "name": "abc-xyz", "x": 1, "items": [1, 2] }),
                None,
            ),
            (
                "status_500_without_header",
                json!({ "status": 500, "name": "zzz", "x": 0, "items": [] }),
                None,
            ),
        ];

        for (ctx_name, response, headers_owned) in contexts {
            let headers_ref = headers_owned.as_ref();
            for expr in cases {
                let conservative = rewrite_assertion_expression_fixed_point_with_mode(
                    expr,
                    NormalizationMode::Conservative,
                );
                let ast = rewrite_assertion_expression_fixed_point_with_mode(
                    expr,
                    NormalizationMode::AstCanonical,
                );

                let before = engine.evaluate(expr, &response, headers_ref, None).unwrap();
                let after_conservative = engine
                    .evaluate(&conservative, &response, headers_ref, None)
                    .unwrap();
                let after_ast = engine.evaluate(&ast, &response, headers_ref, None).unwrap();

                let before_outcome = outcome_of(&before);
                let conservative_outcome = outcome_of(&after_conservative);
                let ast_outcome = outcome_of(&after_ast);

                assert_eq!(
                    before_outcome, conservative_outcome,
                    "conservative rewrite changed outcome in {ctx_name}: {expr} -> {conservative}",
                );
                assert_eq!(
                    before_outcome, ast_outcome,
                    "ast rewrite changed outcome in {ctx_name}: {expr} -> {ast}",
                );

                let conservative_twice = rewrite_assertion_expression_fixed_point_with_mode(
                    &conservative,
                    NormalizationMode::Conservative,
                );
                let ast_twice = rewrite_assertion_expression_fixed_point_with_mode(
                    &ast,
                    NormalizationMode::AstCanonical,
                );
                assert_eq!(
                    conservative, conservative_twice,
                    "conservative rewrite not idempotent in {ctx_name}: {expr}",
                );
                assert_eq!(
                    ast, ast_twice,
                    "ast rewrite not idempotent in {ctx_name}: {expr}",
                );

                let default_path = rewrite_assertion_expression_fixed_point(expr);
                assert_eq!(
                    default_path, ast,
                    "default rewrite diverged from ast mode in {ctx_name}: {expr}",
                );
            }
        }
    }

    #[test]
    fn test_optimizer_hints_preserve_execution_result() {
        use apif_assert::engine::{AssertionEngine, AssertionResult};
        use serde_json::json;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Outcome {
            Pass,
            Fail,
            Error,
        }

        fn outcome_of(result: &AssertionResult) -> Outcome {
            match result {
                AssertionResult::Pass => Outcome::Pass,
                AssertionResult::Fail { .. } => Outcome::Fail,
                AssertionResult::Error(_) => Outcome::Error,
            }
        }

        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
@has_header("x") == false
false == @has_header("x")
@has_header("x") != true
!!@has_header("x")
not not @has_header("x")
.name startswith "abc"
3 > 2
.user.id == .user.id
$user_id != $user_id
if true then "always" else "never" end
if .x > 0 then "same" else "same" end
if @has_header("x") then true else false end
if .status == 200 then false else true end
@len(.items) == 0
(@has_header("x"))
not (.status == 200)
@len(.items) >= 0
@has_header("x") or true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert!(!hints.is_empty());

        let engine = AssertionEngine::with_registry(std::sync::Arc::new(
            apif_plugins::PluginManager::new(),
        ));
        let contexts = vec![
            (
                "status_200_with_header",
                json!({ "status": 200, "name": "abc-xyz", "x": 1, "items": [1, 2], "user": { "id": 1 } }),
                Some(std::collections::HashMap::from([(
                    "x".to_string(),
                    "1".to_string(),
                )])),
            ),
            (
                "status_200_without_header",
                json!({ "status": 200, "name": "abc-xyz", "x": 1, "items": [1, 2], "user": { "id": 1 } }),
                None,
            ),
            (
                "status_500_without_header",
                json!({ "status": 500, "name": "zzz", "x": 0, "items": [], "user": { "id": 1 } }),
                None,
            ),
        ];

        for hint in hints {
            for (ctx_name, response, headers_owned) in &contexts {
                let headers_ref = headers_owned.as_ref();
                let before = engine
                    .evaluate(&hint.before, response, headers_ref, None)
                    .unwrap();
                let after = engine
                    .evaluate(&hint.after, response, headers_ref, None)
                    .unwrap();

                assert_eq!(
                    outcome_of(&before),
                    outcome_of(&after),
                    "rule {} changed outcome in {ctx_name}: '{}' -> '{}'",
                    hint.rule_id,
                    hint.before,
                    hint.after,
                );
            }
        }
    }

    // ─── Redundant type cast tests ───────────────────────────────────

    #[test]
    fn test_suggest_redundant_type_cast_len_uint() {
        let expr = "@len(.items):uint >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures);
        assert!(result.is_some(), "Expected redundant cast for @len(:uint)");
        if let Some((rule_id, rewritten)) = result {
            assert_eq!(rule_id, rule_ids::T002);
            assert_eq!(rewritten, "@len(.items) >= 0");
        }
    }

    #[test]
    fn test_suggest_redundant_type_cast_header_string() {
        // @header returns String, so :string is redundant
        let expr = "@header(\"x\"):string != null";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures);
        assert!(
            result.is_some(),
            "Expected redundant cast for @header(:string)"
        );
        if let Some((rule_id, rewritten)) = result {
            assert_eq!(rule_id, rule_ids::T002);
            assert_eq!(rewritten, "@header(\"x\") != null");
        }
    }

    #[test]
    fn test_suggest_redundant_type_cast_len_to_number() {
        // @len returns UInt, :number is numeric-compatible → redundant
        let expr = "@len(.items):number >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures);
        assert!(
            result.is_some(),
            "Expected redundant cast for @len(:number)"
        );
        if let Some((_, rewritten)) = result {
            assert_eq!(rewritten, "@len(.items) >= 0");
        }
    }

    #[test]
    fn test_suggest_non_redundant_type_cast_number() {
        // .price:number is NOT redundant because .price is Any
        let expr = ".price:number >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures);
        assert!(
            result.is_none(),
            "Should not flag .price:number as redundant"
        );
    }

    #[test]
    fn test_suggest_non_redundant_type_cast_string() {
        // .name:string is NOT redundant because .name is Any
        let expr = ".name:string contains \"hello\"";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures);
        assert!(
            result.is_none(),
            "Should not flag .name:string as redundant"
        );
    }

    #[test]
    fn test_collect_redundant_type_cast_optimization() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items):uint >= 0
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc);
        assert!(!hints.is_empty(), "Expected at least one optimization hint");
        assert_eq!(hints[0].rule_id, rule_ids::T002);
        assert_eq!(hints[0].after, "@len(.items) >= 0");
    }
}
