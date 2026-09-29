use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizeLevel {
    None = 0,
    Layout = 1,
    Safe = 2,
    Advisory = 3,
    Aggressive = 4,
}

impl OptimizeLevel {
    #[must_use]
    pub fn is_enabled(self, rule_level: OptimizeLevel) -> bool {
        self as u8 >= rule_level as u8
    }
}

use apif_parser as parser;
use apif_parser::assertions::strip_assertion_comments;
use apif_plugins::{PluginSignature, TypeInfo, extract_plugin_call_name};
use apif_utils::section_content_line;

fn likely_needs_assertion_rewrite(expr: &str) -> bool {
    expr.contains("==")
        || expr.contains("!=")
        || expr.contains('>')
        || expr.contains('<')
        || expr.contains('@')
        || expr.contains(" startswith ")
        || expr.contains(" endswith ")
        || expr.contains(" startswith(")
        || expr.contains(" endswith(")
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

fn ast_to_if_string(expr: &parser::assertion_ast::AssertionExpr, out: &mut String, prec: u8) {
    use apif_parser::assertion_ast::AssertionExpr;
    {
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
}

fn render_expr(expr: &parser::assertion_ast::AssertionExpr) -> String {
    let mut out = String::new();
    ast_to_if_string(expr, &mut out, 0);
    out
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
            pub const ALL: &'static [RuleId] = &[$(Self::$name),+];

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
    R001 => "OPT_R001",
    R002 => "OPT_R002",
    C001 => "OPT_C001",
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
        preconditions: "expression has form not not <bool-plugin-expr> or !!<bool-plugin-expr>",
        negative_cases: "inner expr is not proven boolean-safe",
        proof_note: "Double negation elimination, in either spelling",
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
        preconditions: "@len(expr) compared to zero; folding to a constant needs Aggressive",
        negative_cases: "comparison is not with zero or not @len plugin",
        proof_note: "Length check simplification: @len(x) == 0 = @is_empty(x). The always-true and \
                     always-false cases (0 <= @len(x), 0 > @len(x), @len(x) < 0) hold only if @len \
                     is total: if the plugin errors on the value, the original fails where the \
                     constant passes",
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
        proof_note: "UInt is always >= 0, so the comparison is always true — provided the plugin \
                     is total: if it errors on the value, the original fails where true passes",
    },
    RewriteRuleMetadata {
        id: rule_ids::T002,
        preconditions: "expression has `:TypeName` suffix and the inner expression already has that type",
        negative_cases: "expression has `:TypeName` but the inner expression has a different or unknown type",
        proof_note: "Type annotation is redundant when the type is already known",
    },
    RewriteRuleMetadata {
        id: rule_ids::R001,
        preconditions: "deprecated plugin call (uuid/email/ip/url/timestamp/empty)",
        negative_cases: "already using canonical name",
        proof_note: "Use canonical plugin name instead of deprecated one",
    },
    RewriteRuleMetadata {
        id: rule_ids::C001,
        preconditions: "expression differs from the canonical form its own AST renders",
        negative_cases: "expression does not parse, so there is no canonical form to offer",
        proof_note: "Canonical spelling: same AST, one spelling — the spelling `fmt` writes",
    },
    RewriteRuleMetadata {
        id: rule_ids::R002,
        preconditions: "negation pattern `!@is_empty(x)`",
        negative_cases: "already using `@has_value`",
        proof_note: "Use `@has_value` instead of `!@is_empty`",
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

static EXTRA_BOOLEAN_PLUGINS: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();

pub fn register_extra_boolean_plugins(names: HashSet<String>) {
    let _ = EXTRA_BOOLEAN_PLUGINS.set(names);
}

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
        || EXTRA_BOOLEAN_PLUGINS
            .get()
            .is_some_and(|extra| extra.contains(plugin_name.as_str()))
}

fn suggest_boolean_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
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

fn suggest_not_not_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
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
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
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

fn suggest_redundant_parens(expr: &str, level: OptimizeLevel) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Layout) {
        return None;
    }
    let trimmed = expr.trim();
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return None;
    }

    let inner = &trimmed[1..trimmed.len() - 1].trim();
    if inner.is_empty() {
        return None;
    }

    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }

    Some((rule_ids::P002, inner.to_string()))
}

fn suggest_double_negation_rewrite(
    expr: &str,
    bool_plugins: &HashSet<String>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
    let trimmed = expr.trim();
    if !trimmed.starts_with("!!") {
        return None;
    }

    let inner = trimmed[2..].trim();
    if is_boolean_plugin_expr(inner, bool_plugins) {
        return Some((rule_ids::B017, inner.to_string()));
    }

    None
}

fn replace_outside_string_literals(expr: &str, needle: &str, replacement: &str) -> Option<String> {
    let mut result = String::with_capacity(expr.len());
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut escaped = false;
    let mut replaced = false;
    let mut chars = expr.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if in_quotes {
            result.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote_char {
                in_quotes = false;
            }
            continue;
        }

        if c == '"' || c == '\'' {
            in_quotes = true;
            quote_char = c;
            result.push(c);
            continue;
        }

        if expr[i..].starts_with(needle) {
            result.push_str(replacement);
            replaced = true;
            let end = i + needle.len();
            while let Some(&(j, _)) = chars.peek() {
                if j < end {
                    chars.next();
                } else {
                    break;
                }
            }
            continue;
        }

        result.push(c);
    }

    if replaced { Some(result) } else { None }
}

fn suggest_operator_canonicalization(expr: &str, level: OptimizeLevel) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Layout) {
        return None;
    }
    for (alias, canonical) in [
        (" startswith ", " startsWith "),
        (" endswith ", " endsWith "),
    ] {
        if let Some(rewritten) = replace_outside_string_literals(expr, alias, canonical) {
            return Some((rule_ids::N001, rewritten));
        }
    }
    let is_operator_grammar = !matches!(
        parser::assertion_ast::parse_assertion(expr.trim()),
        parser::assertion_ast::AssertionExpr::Raw(_)
    );
    if !is_operator_grammar {
        return None;
    }
    for (alias, canonical) in [
        (" startswith(", " startsWith("),
        (" endswith(", " endsWith("),
    ] {
        if let Some(rewritten) = replace_outside_string_literals(expr, alias, canonical) {
            return Some((rule_ids::N001, rewritten));
        }
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

fn suggest_constant_folding(expr: &str, level: OptimizeLevel) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Aggressive) {
        return None;
    }
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
            .is_some_and(|sig| sig.idempotent);
    }

    false
}

fn suggest_reflexive_idempotent(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Aggressive) {
        return None;
    }
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

fn if_parts(ast: &parser::assertion_ast::AssertionExpr) -> Option<(String, String, String)> {
    use parser::assertion_ast::AssertionExpr;

    match ast {
        AssertionExpr::IfThenElse {
            condition,
            then_branch,
            else_branch,
        } => Some((
            render_expr(condition),
            render_expr(then_branch),
            render_expr(else_branch),
        )),
        _ => None,
    }
}

fn suggest_dead_branch_elimination(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
    let (condition, then_expr, else_expr) = if_parts(ast)?;

    if condition == "true" {
        return Some((rule_ids::I001, then_expr));
    }

    if condition == "false" {
        return Some((rule_ids::I001, else_expr));
    }

    None
}

fn suggest_branch_merging(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    let (_condition, then_expr, else_expr) = if_parts(ast)?;

    if then_expr == else_expr {
        return Some((rule_ids::I002, then_expr));
    }

    None
}

fn suggest_nested_if_simplification(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    use parser::assertion_ast::AssertionExpr;

    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    let AssertionExpr::IfThenElse {
        condition,
        then_branch,
        else_branch,
    } = ast
    else {
        return None;
    };

    let inner = match &**then_branch {
        AssertionExpr::Paren(inner) => &**inner,
        other => other,
    };
    let (inner_cond, inner_then, _inner_else) = if_parts(inner)?;
    let (outer_cond, else_expr) = (render_expr(condition), render_expr(else_branch));

    if outer_cond == inner_cond {
        let result = format!(
            "if {} then {} else {} end",
            outer_cond, inner_then, else_expr
        );
        return Some((rule_ids::I003, result));
    }

    None
}

fn suggest_boolean_simplification(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    let (condition, then_expr, else_expr) = if_parts(ast)?;

    if then_expr == "true" && else_expr == "false" {
        return Some((rule_ids::I004, condition));
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

fn suggest_condition_inversion(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    let (condition, then_expr, else_expr) = if_parts(ast)?;

    if then_expr == "false" && else_expr == "true" {
        Some((rule_ids::I005, negate_condition_expr(&condition)))
    } else {
        None
    }
}

fn bool_literal(expr: &parser::assertion_ast::AssertionExpr) -> Option<bool> {
    use parser::assertion_ast::{AssertionExpr, Expr, Literal};
    match expr {
        AssertionExpr::Atom(Expr::Literal(Literal::Bool(b))) => Some(*b),
        AssertionExpr::Paren(inner) => bool_literal(inner),
        _ => None,
    }
}

fn suggest_boolean_identity_laws(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    use parser::assertion_ast::{AssertionExpr, assertion_to_string};

    match ast {
        AssertionExpr::Or { left, right } => {
            if bool_literal(left) == Some(true) || bool_literal(right) == Some(true) {
                return Some((rule_ids::B009, "true".to_string()));
            }
            if bool_literal(right) == Some(false) {
                return Some((rule_ids::B009, assertion_to_string(left)));
            }
            if bool_literal(left) == Some(false) {
                return Some((rule_ids::B009, assertion_to_string(right)));
            }
            None
        }
        AssertionExpr::And { left, right } => {
            if bool_literal(left) == Some(true) {
                return Some((rule_ids::B010, assertion_to_string(right)));
            }
            if bool_literal(right) == Some(true) {
                return Some((rule_ids::B010, assertion_to_string(left)));
            }
            if bool_literal(left) == Some(false) || bool_literal(right) == Some(false) {
                return Some((rule_ids::B010, "false".to_string()));
            }
            None
        }
        _ => None,
    }
}

fn suggest_plugin_length_simplification(
    expr: &str,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Advisory) {
        return None;
    }
    fn extract_len_inner(s: &str) -> Option<&str> {
        let rest = s.strip_prefix("@len(")?;
        let mut depth = 1usize;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return if i == rest.len() - 1 {
                            Some(&rest[..i])
                        } else {
                            None
                        };
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn rewrite_len_zero_cmp(
        op: &str,
        inner: &str,
        len_on_left: bool,
        level: OptimizeLevel,
    ) -> Option<String> {
        let folds_to_a_constant = level.is_enabled(OptimizeLevel::Aggressive);
        match (op, len_on_left) {
            ("==", _) => Some(format!("@is_empty({})", inner)),
            ("<=", true) => Some(format!("@is_empty({})", inner)),
            ("<=", false) => folds_to_a_constant.then(|| "true".to_string()),
            ("!=", _) => Some(format!("@len({}) > 0", inner)),
            (">", true) => None,
            (">", false) => folds_to_a_constant.then(|| "false".to_string()),
            ("<", true) => folds_to_a_constant.then(|| "false".to_string()),
            ("<", false) => None,
            _ => None,
        }
    }

    let expr = expr.trim();

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
                return rewrite_len_zero_cmp(op_name, inner, true, level)
                    .map(|rewrite| (rule_ids::P001, rewrite));
            }

            if left == "0"
                && let Some(inner) = extract_len_inner(right)
            {
                return rewrite_len_zero_cmp(op_name, inner, false, level)
                    .map(|rewrite| (rule_ids::P001, rewrite));
            }
        }
    }

    None
}

fn suggest_type_aware_numeric_comparison(
    expr: &str,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Aggressive) {
        return None;
    }
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

fn top_level_positions(expr: &str, op: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let bytes = expr.as_bytes();

    for (i, ch) in expr.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if depth == 0 && bytes[i..].starts_with(op.as_bytes()) => out.push(i),
            _ => {}
        }
    }
    out
}

fn split_call_args(after_open_paren: &str) -> Option<(&str, &str)> {
    let mut parens = 0i32;
    let mut brackets = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (i, ch) in after_open_paren.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => parens += 1,
            '[' | '{' => brackets += 1,
            ']' | '}' => brackets = (brackets - 1).max(0),
            ')' if parens == 0 && brackets == 0 => {
                return Some((&after_open_paren[..i], after_open_paren[i + 1..].trim()));
            }
            ')' => parens = (parens - 1).max(0),
            _ => {}
        }
    }
    None
}

fn suggest_comparison_negation(
    ast: &parser::assertion_ast::AssertionExpr,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
    use parser::assertion_ast::{AssertionExpr, BinaryOp};

    let AssertionExpr::Not(inner) = ast else {
        return None;
    };
    let inner = match &**inner {
        AssertionExpr::Paren(p) => &**p,
        other => other,
    };
    let AssertionExpr::Binary { op, left, right } = inner else {
        return None;
    };
    let negated = match op {
        BinaryOp::Eq => BinaryOp::Ne,
        BinaryOp::Ne => BinaryOp::Eq,
        BinaryOp::Gt => BinaryOp::Le,
        BinaryOp::Lt => BinaryOp::Ge,
        BinaryOp::Ge => BinaryOp::Lt,
        BinaryOp::Le => BinaryOp::Gt,
        _ => return None,
    };
    let rewritten = parser::assertion_ast::assertion_to_string(&AssertionExpr::Binary {
        op: negated,
        left: left.clone(),
        right: right.clone(),
    });
    Some((rule_ids::N002, rewritten))
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
        if let Some(&op_pos) = top_level_positions(inner, op).first() {
            let left = inner[..op_pos].trim();
            let right = inner[op_pos + op.len()..].trim();

            if !left.is_empty() && !right.is_empty() {
                return Some(format!("{}{}{}", left, neg_op, right));
            }
        }
    }

    None
}

fn suggest_redundant_type_cast(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
    let colon_pos = expr.rfind(':')?;
    if colon_pos == 0 {
        return None;
    }

    let cast_type_name = &expr[colon_pos + 1..];
    let inner_expr = expr[..colon_pos].trim();

    let cast_type_end = cast_type_name
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(cast_type_name.len());
    let cast_type_name = &cast_type_name[..cast_type_end];
    if cast_type_name.is_empty() {
        return None;
    }

    let cast_type = TypeInfo::parse_type_name(cast_type_name)?;

    let inner_tokens = parser::tokenizer::tokenize_assertion(inner_expr);
    let empty_vars = std::collections::HashMap::new();
    let inner_type = apif_semantics::infer_type_from_tokens(&inner_tokens, signatures, &empty_vars);

    if inner_type == TypeInfo::Any || inner_type == TypeInfo::Yaml || inner_type == TypeInfo::Json {
        return None;
    }

    let cast_base = cast_type.base_type();
    let inner_base = inner_type.base_type();

    let types_match =
        cast_base == inner_base || (cast_base.is_numeric() && inner_base.is_numeric());

    if !types_match {
        return None;
    }

    let after_colon = &expr[colon_pos + 1..];
    let rest = after_colon[cast_type_name.len()..].trim();

    let rewritten = if rest.is_empty() {
        inner_expr.to_string()
    } else {
        format!("{} {}", inner_expr, rest)
    };

    Some((rule_ids::T002, rewritten))
}

fn suggest_deprecated_plugin_rename(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }
    let trimmed = expr.trim();

    if let Some(inner) = trimmed.strip_prefix("!@is_empty(")
        && let Some((args, rest)) = split_call_args(inner)
        && rest.is_empty()
    {
        return Some((rule_ids::R002, format!("@has_value({})", args)));
    }

    if let Some(inner) = trimmed.strip_prefix("@is_empty(")
        && let Some((args, rest)) = split_call_args(inner)
        && rest == "== false"
    {
        return Some((rule_ids::R002, format!("@has_value({})", args)));
    }

    if let Some(inner) = trimmed.strip_prefix("false == @is_empty(")
        && let Some((args, rest)) = split_call_args(inner)
        && rest.is_empty()
    {
        return Some((rule_ids::R002, format!("@has_value({})", args)));
    }

    for (name, sig) in signatures {
        let Some(replacement) = sig.replacement else {
            continue;
        };
        let at_name = format!("@{}", name);
        if let Some(rest) = trimmed.strip_prefix(&at_name)
            && rest.starts_with('(')
        {
            return Some((rule_ids::R001, format!("@{}{}", replacement, rest)));
        }
        let not_at_name = format!("!@{}", name);
        if let Some(rest) = trimmed.strip_prefix(&not_at_name)
            && let Some(inner) = rest.strip_prefix('(')
            && let Some((args, after)) = split_call_args(inner)
            && after.is_empty()
        {
            if replacement == "is_empty" {
                return Some((rule_ids::R002, format!("@has_value({})", args)));
            }
            return Some((rule_ids::R001, format!("!@{}{}", replacement, rest)));
        }
    }

    None
}

fn rewrite_assertion_expression_with_context(
    expr: &str,
    signatures: &HashMap<String, PluginSignature>,
    bool_plugins: &HashSet<String>,
    normalization_mode: NormalizationMode,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if !level.is_enabled(OptimizeLevel::Layout) {
        return None;
    }

    let trimmed = expr.trim();
    let parsed = parser::assertion_ast::parse_assertion(trimmed);
    let ast = match normalization_mode {
        #[cfg(test)]
        NormalizationMode::Conservative => parsed,
        NormalizationMode::AstCanonical => parser::assertion_ast::remove_redundant_parens(&parsed),
    };

    if matches!(normalization_mode, NormalizationMode::AstCanonical) {
        let canonical = render_expr(&ast);
        if canonical != trimmed
            && (level.is_enabled(OptimizeLevel::Safe) || only_layout_differs(trimmed, &canonical))
        {
            return Some((rule_ids::C001, canonical));
        }
    }

    if let Some((rule_id, rewrite)) =
        apply_rules_to_expression(trimmed, &ast, signatures, bool_plugins, level)
    {
        return Some((rule_id, rewrite));
    }

    if !level.is_enabled(OptimizeLevel::Safe) {
        return None;
    }

    rewrite_a_subexpression(trimmed, &ast, signatures, bool_plugins, level)
}

pub fn only_layout_differs(before: &str, after: &str) -> bool {
    fn tokens(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for c in text.chars() {
            match quote {
                Some(open) => {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == open {
                        quote = None;
                    }
                }
                None => {
                    if c == '"' || c == '\'' {
                        quote = Some(c);
                        out.push(c);
                    } else if !c.is_whitespace() && c != '(' && c != ')' {
                        out.push(c);
                    }
                }
            }
        }
        out
    }
    tokens(before) == tokens(after)
}

fn rewrite_a_subexpression(
    expr: &str,
    ast: &parser::assertion_ast::AssertionExpr,
    signatures: &HashMap<String, PluginSignature>,
    bool_plugins: &HashSet<String>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    use parser::assertion_ast::AssertionExpr;

    fn child_rewrite(
        node: &AssertionExpr,
        signatures: &HashMap<String, PluginSignature>,
        bool_plugins: &HashSet<String>,
        level: OptimizeLevel,
    ) -> Option<(RuleId, AssertionExpr)> {
        if !matches!(node, AssertionExpr::Paren(_)) {
            let rendered = render_expr(node);
            if let Some((rule_id, rewrite)) =
                apply_rules_to_expression(&rendered, node, signatures, bool_plugins, level)
            {
                let rewritten = rewrite.trim();
                if rewritten != rendered {
                    let parsed = parser::assertion_ast::parse_assertion(rewritten);
                    if !matches!(parsed, AssertionExpr::Raw(_)) {
                        return Some((rule_id, parsed));
                    }
                }
            }
        }

        let rebuild_pair = |left: &AssertionExpr,
                            right: &AssertionExpr,
                            build: &dyn Fn(AssertionExpr, AssertionExpr) -> AssertionExpr|
         -> Option<(RuleId, AssertionExpr)> {
            if let Some((rule_id, new_left)) = child_rewrite(left, signatures, bool_plugins, level)
            {
                return Some((rule_id, build(new_left, right.clone())));
            }
            let (rule_id, new_right) = child_rewrite(right, signatures, bool_plugins, level)?;
            Some((rule_id, build(left.clone(), new_right)))
        };

        match node {
            AssertionExpr::And { left, right } => {
                rebuild_pair(left, right, &|l, r| AssertionExpr::And {
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AssertionExpr::Or { left, right } => {
                rebuild_pair(left, right, &|l, r| AssertionExpr::Or {
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AssertionExpr::Xor { left, right } => {
                rebuild_pair(left, right, &|l, r| AssertionExpr::Xor {
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AssertionExpr::Binary { op, left, right } => {
                let op = *op;
                rebuild_pair(left, right, &move |l, r| AssertionExpr::Binary {
                    op,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AssertionExpr::Not(inner) => child_rewrite(inner, signatures, bool_plugins, level)
                .map(|(rule_id, new_inner)| (rule_id, AssertionExpr::Not(Box::new(new_inner)))),
            AssertionExpr::NotNot(inner) => child_rewrite(inner, signatures, bool_plugins, level)
                .map(|(rule_id, new_inner)| (rule_id, AssertionExpr::NotNot(Box::new(new_inner)))),
            AssertionExpr::Paren(inner) => child_rewrite(inner, signatures, bool_plugins, level)
                .map(|(rule_id, new_inner)| (rule_id, AssertionExpr::Paren(Box::new(new_inner)))),
            AssertionExpr::IfThenElse {
                condition,
                then_branch,
                else_branch,
            } => {
                let rebuild = |c: AssertionExpr, t: AssertionExpr, e: AssertionExpr| {
                    AssertionExpr::IfThenElse {
                        condition: Box::new(c),
                        then_branch: Box::new(t),
                        else_branch: Box::new(e),
                    }
                };
                if let Some((rule_id, new_condition)) =
                    child_rewrite(condition, signatures, bool_plugins, level)
                {
                    return Some((
                        rule_id,
                        rebuild(
                            new_condition,
                            (**then_branch).clone(),
                            (**else_branch).clone(),
                        ),
                    ));
                }
                if let Some((rule_id, new_then)) =
                    child_rewrite(then_branch, signatures, bool_plugins, level)
                {
                    return Some((
                        rule_id,
                        rebuild((**condition).clone(), new_then, (**else_branch).clone()),
                    ));
                }
                let (rule_id, new_else) =
                    child_rewrite(else_branch, signatures, bool_plugins, level)?;
                Some((
                    rule_id,
                    rebuild((**condition).clone(), (**then_branch).clone(), new_else),
                ))
            }
            AssertionExpr::Atom(_) | AssertionExpr::Raw(_) => None,
        }
    }

    if matches!(ast, AssertionExpr::Atom(_) | AssertionExpr::Raw(_)) {
        return None;
    }

    let (rule_id, rewritten) = child_rewrite(ast, signatures, bool_plugins, level)?;
    let settled = parser::assertion_ast::remove_redundant_parens(&rewritten);
    let rendered = render_expr(&settled);
    (rendered != expr).then_some((rule_id, rendered))
}

fn apply_rules_to_expression(
    expr: &str,
    ast: &parser::assertion_ast::AssertionExpr,
    signatures: &HashMap<String, PluginSignature>,
    bool_plugins: &HashSet<String>,
    level: OptimizeLevel,
) -> Option<(RuleId, String)> {
    if let Some((rule_id, rewrite)) = suggest_boolean_rewrite(expr, bool_plugins, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_not_not_rewrite(expr, bool_plugins, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_inequality_rewrite(expr, bool_plugins, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_double_negation_rewrite(expr, bool_plugins, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_operator_canonicalization(expr, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_constant_folding(expr, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_reflexive_idempotent(expr, signatures, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_redundant_parens(expr, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_dead_branch_elimination(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_branch_merging(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_nested_if_simplification(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_boolean_simplification(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_condition_inversion(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_boolean_identity_laws(ast, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_plugin_length_simplification(expr, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_type_aware_numeric_comparison(expr, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_redundant_type_cast(expr, signatures, level) {
        return Some((rule_id, rewrite));
    }

    if let Some((rule_id, rewrite)) = suggest_deprecated_plugin_rename(expr, signatures, level) {
        return Some((rule_id, rewrite));
    }

    suggest_comparison_negation(ast, level)
}

fn rewrite_assertion_expression_fixed_point_with_mode(
    expr: &str,
    mode: NormalizationMode,
    level: OptimizeLevel,
) -> String {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();

    let mut seen: HashSet<String> = HashSet::new();
    let mut current = expr.trim().to_string();
    seen.insert(current.clone());

    while let Some((_, rewritten)) =
        rewrite_assertion_expression_with_context(&current, signatures, bool_plugins, mode, level)
    {
        let normalized = rewritten.trim().to_string();
        if normalized == current {
            break;
        }
        if !seen.insert(normalized.clone()) {
            debug_assert!(
                false,
                "the rules cycle on `{expr}`: `{current}` and `{normalized}` rewrite into each other"
            );
            break;
        }
        current = normalized;
    }

    current
}

pub fn rewrite_assertion_expression_with_level(
    expr: &str,
    level: OptimizeLevel,
) -> Option<(&'static str, String)> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    rewrite_assertion_expression_with_context(
        expr,
        signatures,
        bool_plugins,
        normalization_mode(),
        level,
    )
    .map(|(rule_id, rewrite)| (rule_id.as_str(), rewrite))
}

pub fn rewrite_assertion_expression_fixed_point(expr: &str) -> String {
    rewrite_assertion_expression_fixed_point_with_level(expr, OptimizeLevel::Advisory)
}

pub fn rewrite_assertion_expression_fixed_point_with_level(
    expr: &str,
    level: OptimizeLevel,
) -> String {
    rewrite_assertion_expression_fixed_point_with_mode(expr, normalization_mode(), level)
}

pub fn rewrite_assertion_expression_fixed_point_if_changed(expr: &str) -> Option<String> {
    rewrite_assertion_expression_fixed_point_if_changed_with_level(expr, OptimizeLevel::Advisory)
}

pub fn rewrite_assertion_expression_fixed_point_if_changed_with_level(
    expr: &str,
    level: OptimizeLevel,
) -> Option<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() || !likely_needs_assertion_rewrite(trimmed) {
        None
    } else {
        let rewritten = rewrite_assertion_expression_fixed_point_with_level(trimmed, level);
        if rewritten == trimmed {
            None
        } else {
            Some(rewritten)
        }
    }
}

pub fn collect_assertion_optimizations(
    doc: &parser::GctfDocument,
    level: OptimizeLevel,
) -> Vec<OptimizationHint> {
    let signatures = plugin_signatures();
    let bool_plugins = boolean_plugins();
    let mode = normalization_mode();
    let mut hints = Vec::new();

    for section in doc.iter_chain().flat_map(|d| d.sections.iter()) {
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

            if let Some((rule_id, rewrite)) = rewrite_assertion_expression_with_context(
                &trimmed,
                signatures,
                bool_plugins,
                mode,
                level,
            ) {
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

    fn if_parts_of(expr: &str) -> Option<(String, String, String)> {
        if_parts(&ast(expr))
    }

    fn ast(expr: &str) -> parser::assertion_ast::AssertionExpr {
        parser::assertion_ast::remove_redundant_parens(&parser::assertion_ast::parse_assertion(
            expr.trim(),
        ))
    }

    #[test]
    fn compound_negation_is_declined_rather_than_botched() {
        assert_eq!(
            suggest_comparison_negation(&ast("!(.x == 1 and .y == 2)"), OptimizeLevel::Safe),
            None
        );
        assert_eq!(
            suggest_comparison_negation(&ast("not (.x == 1 or .y == 2)"), OptimizeLevel::Safe),
            None
        );
        assert_eq!(
            suggest_comparison_negation(&ast("!(.x == 1)"), OptimizeLevel::Safe)
                .map(|(_, out)| out)
                .as_deref(),
            Some(".x != 1")
        );
    }

    #[test]
    fn a_comparison_operator_inside_parens_is_not_the_top_level_one() {
        assert_eq!(
            negate_comparison_expr("(.a == 1) != .b").as_deref(),
            Some("(.a == 1) == .b")
        );
    }

    #[test]
    fn is_empty_rename_requires_its_own_parenthesis_to_close() {
        let sigs = HashMap::new();
        assert_eq!(
            suggest_deprecated_plugin_rename(
                "@is_empty(.a) and @is_empty(.b) == false",
                &sigs,
                OptimizeLevel::Safe
            ),
            None
        );
        assert_eq!(
            suggest_deprecated_plugin_rename("!@is_empty(.a) and .b", &sigs, OptimizeLevel::Safe),
            None
        );
        for (input, expected) in [
            ("@is_empty(.a) == false", "@has_value(.a)"),
            ("!@is_empty(.a)", "@has_value(.a)"),
            ("false == @is_empty(.a)", "@has_value(.a)"),
            ("!@is_empty(f(.a, g(.b)))", "@has_value(f(.a, g(.b)))"),
        ] {
            assert_eq!(
                suggest_deprecated_plugin_rename(input, &sigs, OptimizeLevel::Safe)
                    .map(|(_, out)| out)
                    .as_deref(),
                Some(expected),
                "input: {input}"
            );
        }
    }

    #[test]
    fn if_rules_see_conditionals_whose_branches_are_paths() {
        for (input, rule, rewrite) in [
            ("if true then .a else .b end", "OPT_I001", ".a"),
            ("if false then .a else .b end", "OPT_I001", ".b"),
            ("if .x then .a else .a end", "OPT_I002", ".a"),
            (
                "if .x then if .x then .a else .b end else .c end",
                "OPT_I003",
                "if .x then .a else .c end",
            ),
            ("if .a then true else false end", "OPT_I004", ".a"),
            ("if .a then false else true end", "OPT_I005", "!.a"),
        ] {
            assert_eq!(
                rewrite_assertion_expression_with_level(input, OptimizeLevel::Aggressive),
                Some((rule, rewrite.to_string())),
                "{input}"
            );
        }
    }

    #[test]
    fn boolean_identities_respect_operator_precedence() {
        assert_eq!(
            suggest_boolean_identity_laws(&ast(".a or .b and false"), OptimizeLevel::Advisory),
            None
        );
        assert_eq!(
            suggest_boolean_identity_laws(&ast(".a or .b and true"), OptimizeLevel::Advisory),
            None
        );
        assert_eq!(
            suggest_boolean_identity_laws(&ast(".a and (.b or false)"), OptimizeLevel::Advisory),
            None
        );
        for (input, expected) in [
            (".a or true", "true"),
            (".a or false", ".a"),
            (".a and true", ".a"),
            (".a and false", "false"),
        ] {
            assert_eq!(
                suggest_boolean_identity_laws(&ast(input), OptimizeLevel::Advisory)
                    .map(|(_, out)| out)
                    .as_deref(),
                Some(expected),
                "input: {input}"
            );
        }
    }

    #[test]
    fn call_args_end_at_the_call_s_own_parenthesis() {
        assert_eq!(split_call_args(".a)"), Some((".a", "")));
        assert_eq!(split_call_args(".a) == false"), Some((".a", "== false")));
        assert_eq!(split_call_args("f(.a, g(.b)))"), Some(("f(.a, g(.b))", "")));
        assert_eq!(split_call_args("[(.a), .b])"), Some(("[(.a), .b]", "")));
        assert_eq!(split_call_args(r#"".a)")"#), Some((r#"".a)""#, "")));
        assert_eq!(split_call_args(".a"), None);
    }

    #[test]
    fn a_boolean_operator_inside_a_string_is_not_an_operator() {
        assert!(top_level_positions(r#".msg == " and ""#, " and ").is_empty());
        assert!(top_level_positions(".a and .b", " and ").len() == 1);
        assert!(top_level_positions("f(.a and .b)", " and ").is_empty());
    }

    fn ast_mode_active_for_tests() -> bool {
        matches!(normalization_mode(), NormalizationMode::AstCanonical)
    }

    #[test]
    fn collect_assertion_optimizations_detects_boolean_rewrite() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x-request-id") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B001);
        assert_eq!(hints[0].after, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn collect_assertion_optimizations_finds_second_document_in_chain() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.ok == true

--- ENDPOINT ---
test.Service/Method2

--- ASSERTS ---
@has_header("x-request-id") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        assert!(!doc.is_single_document(), "fixture must actually chain");
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1, "{hints:?}");
        assert_eq!(hints[0].rule_id, rule_ids::B001);
    }

    #[test]
    fn collect_assertion_optimizations_detects_double_negation_rewrite() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@has_header("x-request-id")
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::C001);
        assert_eq!(hints[0].after, "not not @has_header(\"x-request-id\")");
        assert_eq!(
            rewrite_assertion_expression_fixed_point("!!@has_header(\"x-request-id\")"),
            "@has_header(\"x-request-id\")"
        );
    }

    #[test]
    fn collect_assertion_optimizations_detects_operator_canonicalization() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.name startswith "abc"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].after, ".name startsWith \"abc\"");
        assert_eq!(
            hints[0].rule_id,
            if ast_mode_active_for_tests() {
                rule_ids::C001
            } else {
                rule_ids::N001
            }
        );
    }

    #[test]
    fn collect_assertion_optimizations_no_double_negation_for_non_boolean_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
!!@len(.items)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        for hint in &hints {
            assert_eq!(hint.rule_id, rule_ids::C001, "only a respelling is offered");
        }
        assert_eq!(
            rewrite_assertion_expression_fixed_point("!!@len(.items)"),
            "not not @len(.items)",
            "the negation stays: @len is not a boolean plugin"
        );
    }

    #[test]
    fn collect_assertion_optimizations_constant_fold_numeric_compare() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
1 + 1 == 2
3 > 2
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B006);
        assert_eq!(hints[0].before, "3 > 2");
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn collect_assertion_optimizations_constant_fold_string_equality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
"a" == "a"
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B006);
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn rewrite_rule_metadata_is_complete() {
        for id in RuleId::ALL {
            let meta = rule_metadata(*id).unwrap_or_else(|| panic!("missing metadata for {id}"));
            assert!(!meta.preconditions.is_empty());
            assert!(!meta.negative_cases.is_empty());
            assert!(!meta.proof_note.is_empty());
        }
    }

    #[test]
    fn every_declared_rule_has_an_input_that_produces_it() {
        let inputs: &[(RuleId, &str)] = &[
            (rule_ids::B001, "@is_empty(.a) == true"),
            (rule_ids::B002, "@is_empty(.a) == false"),
            (rule_ids::B003, "true == @is_empty(.a)"),
            (rule_ids::B004, "false == @is_empty(.a)"),
            (rule_ids::B006, "1 < 2"),
            (rule_ids::B007, ".a == .a"),
            (rule_ids::B008, ".a != .a"),
            (rule_ids::B009, ".a or true"),
            (rule_ids::B010, ".a and false"),
            (rule_ids::B013, "@is_empty(.a) != true"),
            (rule_ids::B014, "@is_empty(.a) != false"),
            (rule_ids::B015, "true != @is_empty(.a)"),
            (rule_ids::B016, "false != @is_empty(.a)"),
            (rule_ids::B017, "not not @is_empty(.a)"),
            (rule_ids::N001, ".name startswith \"a\" ="),
            (rule_ids::N002, "!(.a == 1)"),
            (rule_ids::I001, "if true then .a else .b end"),
            (rule_ids::I002, "if .x then .a else .a end"),
            (
                rule_ids::I003,
                "if .x then if .x then .a else .b end else .c end",
            ),
            (rule_ids::I004, "if .a then true else false end"),
            (rule_ids::I005, "if .a then false else true end"),
            (rule_ids::P001, "@len(.a) == 0"),
            (rule_ids::P002, "(.x = 5)"),
            (rule_ids::T001, "@len(.a) >= 0"),
            (rule_ids::T002, "@len(.a):uint"),
            (rule_ids::R001, "@uuid(.a)"),
            (rule_ids::R002, "!@is_empty(.a)"),
            (rule_ids::C001, "(@is_empty(.a)) == true"),
        ];

        for id in RuleId::ALL {
            let (_, input) = inputs
                .iter()
                .find(|(candidate, _)| candidate == id)
                .unwrap_or_else(|| {
                    panic!("{id} has no input in this table — add one or delete the rule")
                });
            let produced =
                rewrite_assertion_expression_with_level(input, OptimizeLevel::Aggressive)
                    .map(|(reported, _)| reported);
            assert_eq!(
                produced,
                Some(id.as_str()),
                "{id} is unreachable: {input} produced {produced:?}"
            );
        }
    }

    #[test]
    fn canonicalization_is_reported_instead_of_discarded() {
        for (input, canonical) in [
            ("(.a) or (.b)", ".a or .b"),
            ("(.a == 1)", ".a == 1"),
            (".name startswith \"abc\"", ".name startsWith \"abc\""),
        ] {
            assert_eq!(
                rewrite_assertion_expression_with_level(input, OptimizeLevel::Safe),
                Some((rule_ids::C001.as_str(), canonical.to_string())),
                "{input}"
            );
        }
    }

    #[test]
    fn a_rule_reaches_an_operand_of_a_compound_expression() {
        for (input, rule, rewrite) in [
            (
                "@len(.a) == 0 and .b",
                rule_ids::P001,
                "@is_empty(.a) and .b",
            ),
            (
                ".b and @len(.a) == 0",
                rule_ids::P001,
                ".b and @is_empty(.a)",
            ),
            (
                "@is_empty(.a) == true and .b == 1",
                rule_ids::B001,
                "@is_empty(.a) and .b == 1",
            ),
            (
                ".a and (.b or @len(.c) == 0)",
                rule_ids::P001,
                ".a and (.b or @is_empty(.c))",
            ),
            (
                "if .c then @uuid(.a) else .b end",
                rule_ids::R001,
                "if .c then @is_uuid(.a) else .b end",
            ),
            (
                "!(@len(.a) == 0) or .b",
                rule_ids::N002,
                "@len(.a) != 0 or .b",
            ),
        ] {
            assert_eq!(
                rewrite_assertion_expression_with_level(input, OptimizeLevel::Advisory),
                Some((rule.as_str(), rewrite.to_string())),
                "{input}"
            );
        }
    }

    #[test]
    fn a_compound_expression_settles_the_way_its_operands_would_alone() {
        for (input, settled) in [
            (
                ".a startswith \"x\" and .b endswith \"y\"",
                ".a startsWith \"x\" and .b endsWith \"y\"",
            ),
            (
                "@is_empty(.a) and @is_empty(.b) == false",
                "@is_empty(.a) and @has_value(.b)",
            ),
            (".a and (.b or false)", ".a and .b"),
            ("!(@len(.a) == 0) or .b", "@len(.a) > 0 or .b"),
        ] {
            assert_eq!(
                rewrite_assertion_expression_fixed_point(input),
                settled,
                "{input}"
            );
        }
    }

    #[test]
    fn the_fixed_point_settles_rather_than_running_out_of_iterations() {
        for input in [
            "@len(.a) == 0 and .b",
            "(@is_empty(.a)) == true",
            "!!@is_uuid(.id)",
            "if .c then (if .c then .a else .b end) else .z end",
            "@is_empty(.a) and @is_empty(.b) == false",
            ".c ? true : false",
            ".a % .b",
        ] {
            for level in [
                OptimizeLevel::Safe,
                OptimizeLevel::Advisory,
                OptimizeLevel::Aggressive,
            ] {
                let once = rewrite_assertion_expression_fixed_point_with_level(input, level);
                let twice = rewrite_assertion_expression_fixed_point_with_level(&once, level);
                assert_eq!(once, twice, "{input} at {level:?} did not settle");
            }
        }
    }

    #[test]
    fn level_none_does_no_work_at_all() {
        for input in [
            "(@is_empty(.a)) == true",
            "@len(.a) == 0 and .b",
            "if true then .a else .b end",
            "!!@is_uuid(.id)",
        ] {
            assert_eq!(
                rewrite_assertion_expression_with_level(input, OptimizeLevel::None),
                None,
                "{input}"
            );
            assert_eq!(
                rewrite_assertion_expression_fixed_point_with_level(input, OptimizeLevel::None),
                input,
                "{input}"
            );
        }
    }

    #[test]
    fn the_ternary_spelling_is_left_alone() {
        for level in [
            OptimizeLevel::Safe,
            OptimizeLevel::Advisory,
            OptimizeLevel::Aggressive,
        ] {
            for input in [".c ? true : false", ".c ? .a : .b"] {
                assert_eq!(
                    rewrite_assertion_expression_with_level(input, level),
                    None,
                    "{input} at {level:?}: the AST does not read this spelling, so there is \
                     no canonical form to offer"
                );
            }
        }
    }

    #[test]
    fn a_rewrite_carries_only_what_its_own_rule_did() {
        assert_eq!(
            rewrite_assertion_expression_with_level(
                "(@is_empty(.a)) == true",
                OptimizeLevel::Advisory
            ),
            Some((rule_ids::C001.as_str(), "@is_empty(.a) == true".to_string())),
            "dropping the parentheses is canonicalization, not OPT_B001"
        );
        assert_eq!(
            rewrite_assertion_expression_with_level(
                "@is_empty(.a) == true",
                OptimizeLevel::Advisory
            ),
            Some((rule_ids::B001.as_str(), "@is_empty(.a)".to_string())),
            "with the parentheses already gone, OPT_B001 is the whole diff"
        );
        assert_eq!(
            rewrite_assertion_expression_fixed_point("(@is_empty(.a)) == true"),
            "@is_empty(.a)"
        );
    }

    #[test]
    fn parentheses_are_dropped_only_when_the_first_one_closes_the_last() {
        assert_eq!(
            suggest_redundant_parens("(.x = 5) or (.y = 6)", OptimizeLevel::Safe),
            None,
            "the outer parentheses are two pairs, not one wrapper"
        );
        assert_eq!(
            suggest_redundant_parens("(.x = 5)", OptimizeLevel::Safe)
                .map(|(_, out)| out)
                .as_deref(),
            Some(".x = 5")
        );
    }

    #[test]
    fn optimization_hint_contains_rule_metadata() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].preconditions.as_deref().is_some());
        assert!(hints[0].negative_cases.as_deref().is_some());
        assert!(hints[0].proof_note.as_deref().is_some());
    }

    #[test]
    fn collect_assertion_optimizations_reflexive_idempotent_path() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
.user.id == .user.id
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B007);
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn collect_assertion_optimizations_no_reflexive_for_non_idempotent_plugin() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@env("HOME") == @env("HOME")
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);

        assert!(hints.is_empty());
    }

    #[test]
    fn collect_assertion_optimizations_reflexive_idempotent_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
$user_id != $user_id
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);

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
    fn collect_assertion_optimizations_ignores_inline_comments() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
true == @has_header("x-request-id") // comment should be ignored
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B003);
        assert_eq!(hints[0].after, "@has_header(\"x-request-id\")");
    }

    #[test]
    fn likely_needs_assertion_rewrite_fast_path() {
        assert!(likely_needs_assertion_rewrite("@scope_message_count()"));
        assert!(likely_needs_assertion_rewrite(
            "@scope.message_count() == 2"
        ));
        assert!(likely_needs_assertion_rewrite("@elapsed_ms() >= 10"));
        assert!(likely_needs_assertion_rewrite("true == @has_header(\"x\")"));
        assert!(likely_needs_assertion_rewrite(".name startswith \"abc\""));
        assert!(likely_needs_assertion_rewrite("if true then 1 else 2 end"));
    }

    #[test]
    fn dead_branch_elimination_true() {
        let (rule_id, rewritten) = suggest_dead_branch_elimination(
            &ast("if true then \"yes\" else \"no\" end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I001);
        assert_eq!(rewritten, "\"yes\"");
    }

    #[test]
    fn dead_branch_elimination_false() {
        let (rule_id, rewritten) = suggest_dead_branch_elimination(
            &ast("if false then \"yes\" else \"no\" end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I001);
        assert_eq!(rewritten, "\"no\"");
    }

    #[test]
    fn branch_merging() {
        let (rule_id, rewritten) = suggest_branch_merging(
            &ast("if .x > 0 then \"same\" else \"same\" end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I002);
        assert_eq!(rewritten, "\"same\"");
    }

    #[test]
    fn nested_if_simplification() {
        let input =
            "if .a > 0 then (if .a > 0 then \"inner\" else \"other\" end) else \"outer\" end";
        let result = suggest_nested_if_simplification(&ast(input), OptimizeLevel::Advisory);
        assert!(result.is_some());
        let (rule_id, rewritten) = result.unwrap();
        assert_eq!(rule_id, rule_ids::I003);
        assert_eq!(rewritten, "if .a > 0 then \"inner\" else \"outer\" end");
    }

    #[test]
    fn parse_if_then_else_simple() {
        let (cond, then_expr, else_expr) =
            if_parts_of("if .x > 0 then \"yes\" else \"no\" end").unwrap();
        assert_eq!(cond, ".x > 0");
        assert_eq!(then_expr, "\"yes\"");
        assert_eq!(else_expr, "\"no\"");
    }

    #[test]
    fn parse_if_then_else_nested() {
        let (cond, then_expr, else_expr) = if_parts_of(
            "if .a > 0 then (if .b > 0 then \"both\" else \"a only\" end) else \"none\" end",
        )
        .unwrap();
        assert_eq!(cond, ".a > 0");
        assert_eq!(
            then_expr, "if .b > 0 then \"both\" else \"a only\" end",
            "the rules see the reduced tree, so the wrapper parentheses are already gone"
        );
        assert_eq!(else_expr, "\"none\"");
    }

    #[test]
    fn collect_optimizations_detects_dead_branch() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if true then "always" else "never" end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::I001);
        assert_eq!(hints[0].after, "\"always\"");
    }

    #[test]
    fn collect_optimizations_detects_branch_merging() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if .x > 0 then "same" else "same" end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::I002);
        assert_eq!(hints[0].after, "\"same\"");
    }

    #[test]
    fn boolean_simplification() {
        let (rule_id, rewritten) = suggest_boolean_simplification(
            &ast("if .x > 0 then true else false end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I004);
        assert_eq!(rewritten, ".x > 0");
    }

    #[test]
    fn condition_inversion() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if .x > 0 then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, ".x <= 0");
    }

    #[test]
    fn condition_inversion_contains_needs_parens() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if .name contains \"foo\" then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(.name contains \"foo\")");
    }

    #[test]
    fn condition_inversion_simple_plugin_call_no_parens() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if @has_header(\"x\") then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!@has_header(\"x\")");
    }

    #[test]
    fn condition_inversion_not_keyword_gets_grouped() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if not @has_header(\"x\") then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(
            rewritten, "!(!@has_header(\"x\"))",
            "the condition comes back in the canonical spelling of `not`"
        );
    }

    #[test]
    fn condition_inversion_bang_gets_grouped() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if !@has_header(\"x\") then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(!@has_header(\"x\"))");
    }

    #[test]
    fn condition_inversion_matches_gets_grouped() {
        let (rule_id, rewritten) = suggest_condition_inversion(
            &ast("if .name matches /foo.*/ then false else true end"),
            OptimizeLevel::Advisory,
        )
        .unwrap();
        assert_eq!(rule_id, rule_ids::I005);
        assert_eq!(rewritten, "!(.name matches /foo.*/)");
    }

    #[test]
    fn collect_optimizations_boolean_simplification() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if @has_header("x") then true else false end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::I004);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_condition_inversion() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
if .status == 200 then false else true end
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::I005);
        assert_eq!(hints[0].after, ".status != 200");
    }

    #[test]
    fn parse_if_then_else_string_with_else_keyword() {
        let (cond, then_expr, else_expr) =
            if_parts_of(r#"if true then " else " else "no" end"#).unwrap();
        assert_eq!(cond, "true");
        assert_eq!(then_expr, r#"" else ""#);
        assert_eq!(else_expr, r#""no""#);
    }

    #[test]
    fn parse_if_then_else_then_in_string_condition() {
        let (cond, then_expr, else_expr) =
            if_parts_of(r#"if .x == "then" then "yes" else "no" end"#).unwrap();
        assert_eq!(cond, r#".x == "then""#);
        assert_eq!(then_expr, r#""yes""#);
        assert_eq!(else_expr, r#""no""#);
    }

    #[test]
    fn boolean_identity_or() {
        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast(".x or true"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, "true");

        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast(".x or false"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, ".x");

        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast("true or .x"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B009);
        assert_eq!(rewritten, "true");
    }

    #[test]
    fn boolean_absorption_and() {
        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast(".x and true"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, ".x");

        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast(".x and false"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, "false");

        let (rule_id, rewritten) =
            suggest_boolean_identity_laws(&ast("false and .x"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::B010);
        assert_eq!(rewritten, "false");
    }

    #[test]
    fn plugin_length_simplification() {
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) == 0", OptimizeLevel::Advisory)
                .unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@is_empty(.items)");

        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) != 0", OptimizeLevel::Advisory)
                .unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@len(.items) > 0");

        let result =
            suggest_plugin_length_simplification("@len(.items) > 0", OptimizeLevel::Advisory);
        assert!(result.is_none());

        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("0 == @len(.items)", OptimizeLevel::Advisory)
                .unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@is_empty(.items)");
    }

    #[test]
    fn plugin_length_le_zero_is_operand_side_aware() {
        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("@len(.items) <= 0", OptimizeLevel::Advisory)
                .unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "@is_empty(.items)");

        assert_eq!(
            suggest_plugin_length_simplification("0 <= @len(.items)", OptimizeLevel::Advisory),
            None,
            "folding to a constant assumes @len never errors, which is an Aggressive assumption"
        );

        let (rule_id, rewritten) =
            suggest_plugin_length_simplification("0 <= @len(.items)", OptimizeLevel::Aggressive)
                .unwrap();
        assert_eq!(rule_id, rule_ids::P001);
        assert_eq!(rewritten, "true");
        assert_ne!(rewritten, "@is_empty(.items)");
    }

    #[test]
    fn a_length_comparison_folds_to_a_constant_only_when_totality_is_assumed() {
        for input in ["0 <= @len(.items)", "0 > @len(.items)", "@len(.items) < 0"] {
            assert_eq!(
                suggest_plugin_length_simplification(input, OptimizeLevel::Advisory),
                None,
                "{input}"
            );
            assert!(
                suggest_plugin_length_simplification(input, OptimizeLevel::Aggressive).is_some(),
                "{input}"
            );
        }
    }

    #[test]
    fn comparison_negation() {
        let (rule_id, rewritten) =
            suggest_comparison_negation(&ast("not (.x == 5)"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x != 5");

        let (rule_id, rewritten) =
            suggest_comparison_negation(&ast("not (.x != 5)"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x == 5");

        let (rule_id, rewritten) =
            suggest_comparison_negation(&ast("not (.x > 5)"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x <= 5");

        let (rule_id, rewritten) =
            suggest_comparison_negation(&ast("not (.x >= 5)"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x < 5");

        let (rule_id, rewritten) =
            suggest_comparison_negation(&ast("!(.x <= 5)"), OptimizeLevel::Advisory).unwrap();
        assert_eq!(rule_id, rule_ids::N002);
        assert_eq!(rewritten, ".x > 5");

        assert!(suggest_comparison_negation(&ast("!(.x)"), OptimizeLevel::Advisory).is_none());
    }

    #[test]
    fn collect_optimizations_boolean_identity() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") or true
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B009);
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn collect_optimizations_plugin_length() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items) == 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::P001);
        assert_eq!(hints[0].after, "@is_empty(.items)");
    }

    #[test]
    fn collect_optimizations_type_aware_uint_gte_zero() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items) >= 0
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Aggressive);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::T001);
        assert_eq!(hints[0].after, "true");
    }

    #[test]
    fn collect_optimizations_comparison_negation() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
not (.status == 200)
"#;

        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::C001);
        assert_eq!(hints[0].after, "!(.status == 200)");
        assert_eq!(
            rewrite_assertion_expression_fixed_point("not (.status == 200)"),
            ".status != 200"
        );
    }

    #[test]
    fn collect_optimizations_b002_expr_equals_false() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") == false
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B002);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b004_false_equals_expr() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
false == @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B004);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b013_inequality_true() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") != true
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B013);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b014_inequality_false() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@has_header("x") != false
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B014);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b015_true_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
true != @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B015);
        assert_eq!(hints[0].after, "!@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b016_false_inequality() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
false != @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B016);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_b017_double_not_word() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
not not @has_header("x")
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].rule_id, rule_ids::B017);
        assert_eq!(hints[0].after, "@has_header(\"x\")");
    }

    #[test]
    fn collect_optimizations_p002_redundant_parens() {
        let result = rewrite_assertion_expression_fixed_point("(@has_header(\"x\"))");
        assert_eq!(result, "@has_header(\"x\")");
    }

    #[test]
    fn boolean_plugins_contains_uuid() {
        let bp = boolean_plugins();
        assert!(bp.contains("uuid"));
        assert!(bp.contains("email"));
        assert!(bp.contains("empty"));
    }

    #[test]
    fn plugin_signatures_returns_map() {
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
    fn suggest_constant_folding_string_equality() {
        let result = suggest_constant_folding("\"foo\" == \"foo\"", OptimizeLevel::Aggressive);
        assert!(result.is_some());
        let (rule_id, after) = result.unwrap();
        assert_eq!(rule_id, rule_ids::B006);
        assert_eq!(after, "true");
    }

    #[test]
    fn suggest_constant_folding_mixed_types() {
        let result = suggest_constant_folding("\"foo\" == 123", OptimizeLevel::Aggressive);
        assert!(result.is_some());
        let (_rule_id, after) = result.unwrap();
        assert_eq!(after, "false");
    }

    #[test]
    fn suggest_constant_folding_invalid_json() {
        let result = suggest_constant_folding("@len(.x) == 5", OptimizeLevel::Aggressive);
        assert!(result.is_none());
    }

    #[test]
    fn normalization_mode_is_ast_canonical() {
        assert_eq!(normalization_mode(), NormalizationMode::AstCanonical);
    }

    #[test]
    fn ast_mode_can_change_first_matching_rule() {
        let signatures = plugin_signatures();
        let bool_plugins = boolean_plugins();
        let expr = "((@has_header(\"x\"))) == true";

        let conservative = rewrite_assertion_expression_with_context(
            expr,
            signatures,
            bool_plugins,
            NormalizationMode::Conservative,
            OptimizeLevel::Advisory,
        );
        let ast = rewrite_assertion_expression_with_context(
            expr,
            signatures,
            bool_plugins,
            NormalizationMode::AstCanonical,
            OptimizeLevel::Advisory,
        );

        assert_eq!(conservative.map(|(id, _)| id), None);
        assert_eq!(ast.map(|(id, _)| id), Some(rule_ids::C001));
        assert_eq!(
            rewrite_assertion_expression_fixed_point(expr),
            "@has_header(\"x\")"
        );
    }

    #[test]
    fn ast_canonical_mode_preserves_execution_result() {
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

        let engine =
            AssertionEngine::with_registry(std::sync::Arc::new(apif_plugins::PluginManager::new()));
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
                    OptimizeLevel::Advisory,
                );
                let ast = rewrite_assertion_expression_fixed_point_with_mode(
                    expr,
                    NormalizationMode::AstCanonical,
                    OptimizeLevel::Advisory,
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
                    OptimizeLevel::Advisory,
                );
                let ast_twice = rewrite_assertion_expression_fixed_point_with_mode(
                    &ast,
                    NormalizationMode::AstCanonical,
                    OptimizeLevel::Advisory,
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
    fn optimizer_hints_preserve_execution_result() {
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
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert!(!hints.is_empty());

        let engine =
            AssertionEngine::with_registry(std::sync::Arc::new(apif_plugins::PluginManager::new()));
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

    #[test]
    fn suggest_redundant_type_cast_len_uint() {
        let expr = "@len(.items):uint >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures, OptimizeLevel::Advisory);
        assert!(result.is_some(), "Expected redundant cast for @len(:uint)");
        if let Some((rule_id, rewritten)) = result {
            assert_eq!(rule_id, rule_ids::T002);
            assert_eq!(rewritten, "@len(.items) >= 0");
        }
    }

    #[test]
    fn suggest_redundant_type_cast_header_string() {
        let expr = "@header(\"x\"):string != null";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures, OptimizeLevel::Advisory);
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
    fn suggest_redundant_type_cast_len_to_number() {
        let expr = "@len(.items):number >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures, OptimizeLevel::Advisory);
        assert!(
            result.is_some(),
            "Expected redundant cast for @len(:number)"
        );
        if let Some((_, rewritten)) = result {
            assert_eq!(rewritten, "@len(.items) >= 0");
        }
    }

    #[test]
    fn suggest_non_redundant_type_cast_number() {
        let expr = ".price:number >= 0";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures, OptimizeLevel::Advisory);
        assert!(
            result.is_none(),
            "Should not flag .price:number as redundant"
        );
    }

    #[test]
    fn suggest_non_redundant_type_cast_string() {
        let expr = ".name:string contains \"hello\"";
        let signatures = plugin_signatures();
        let result = suggest_redundant_type_cast(expr, signatures, OptimizeLevel::Advisory);
        assert!(
            result.is_none(),
            "Should not flag .name:string as redundant"
        );
    }

    #[test]
    fn collect_redundant_type_cast_optimization() {
        let content = r#"--- ENDPOINT ---
test.Service/Method

--- ASSERTS ---
@len(.items):uint >= 0
"#;
        let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
        let hints = collect_assertion_optimizations(&doc, OptimizeLevel::Advisory);
        assert!(!hints.is_empty(), "Expected at least one optimization hint");
        assert_eq!(hints[0].rule_id, rule_ids::T002);
        assert_eq!(hints[0].after, "@len(.items) >= 0");
    }

    #[test]
    fn operator_canonicalization_skips_string_literals() {
        assert_eq!(
            suggest_operator_canonicalization(
                r#".msg == "run startswith now""#,
                OptimizeLevel::Safe
            ),
            None
        );
        assert_eq!(
            suggest_operator_canonicalization(r#".msg == "x endswith y""#, OptimizeLevel::Safe),
            None
        );
        assert_eq!(
            suggest_operator_canonicalization(r#".msg startswith "abc""#, OptimizeLevel::Safe),
            Some((rule_ids::N001, r#".msg startsWith "abc""#.to_string()))
        );
    }

    #[test]
    fn the_call_spelling_of_startswith_is_canonicalized_at_layout_level() {
        assert_eq!(
            suggest_operator_canonicalization(r#".msg startswith("abc")"#, OptimizeLevel::Layout),
            Some((rule_ids::N001, r#".msg startsWith("abc")"#.to_string()))
        );
        assert_eq!(
            suggest_operator_canonicalization(r#".msg endswith("abc")"#, OptimizeLevel::Layout),
            Some((rule_ids::N001, r#".msg endsWith("abc")"#.to_string()))
        );
        assert_eq!(
            suggest_operator_canonicalization(
                r#".msg == "a startswith(b)""#,
                OptimizeLevel::Layout
            ),
            None
        );
        assert_eq!(
            suggest_operator_canonicalization(r#".msg startswith "abc""#, OptimizeLevel::None),
            None
        );
        for jq in [r#".a | endswith("x")"#, r#".b | startswith("q")"#] {
            assert_eq!(
                suggest_operator_canonicalization(jq, OptimizeLevel::Aggressive),
                None,
                "{jq} is a jq pipeline and endswith/startswith are jq builtins there"
            );
            assert_eq!(
                rewrite_assertion_expression_fixed_point_with_level(jq, OptimizeLevel::Aggressive),
                jq
            );
        }
    }

    #[test]
    fn layout_level_keeps_meaning_and_only_moves_whitespace_and_parens() {
        for kept in [
            "if true then .a else .b end",
            "not (.x == 1 or .y == 2)",
            "!(.x == 1)",
            "@is_uuid(.id) == true",
            "!!@is_uuid(.id)",
            "@len(.x) == 0",
        ] {
            assert_eq!(
                rewrite_assertion_expression_fixed_point_with_level(kept, OptimizeLevel::Layout),
                kept,
                "layout level rewrote the meaning of {kept}"
            );
        }
        assert_eq!(
            rewrite_assertion_expression_fixed_point_with_level("(.a)", OptimizeLevel::Layout),
            ".a"
        );
        assert_eq!(
            rewrite_assertion_expression_fixed_point_with_level(".a==1", OptimizeLevel::Layout),
            ".a == 1"
        );
        assert!(only_layout_differs("(.a  ==1)", ".a == 1"));
        assert!(!only_layout_differs("not (.a == 1)", "!(.a == 1)"));
        assert!(!only_layout_differs("\"a b\"", "\"ab\""));
        assert!(!only_layout_differs(".a == \"( x )\"", ".a == \"(x)\""));
        assert!(only_layout_differs(".a==\"a b\"", ".a == \"a b\""));
        assert!(!only_layout_differs(
            r#".a == "say \" hi""#,
            r#".a == "say \"hi""#
        ));
        assert!(only_layout_differs(".a  == 'a  b'", ".a == 'a  b'"));
    }

    #[test]
    fn len_zero_simplification_requires_whole_lhs() {
        assert_eq!(
            suggest_plugin_length_simplification(
                "@len(a) and @len(b) == 0",
                OptimizeLevel::Advisory
            ),
            None
        );
        assert_eq!(
            suggest_plugin_length_simplification("@len(.x) == 0", OptimizeLevel::Advisory),
            Some((rule_ids::P001, "@is_empty(.x)".to_string()))
        );
        assert_eq!(
            suggest_plugin_length_simplification("@len(f(.x)) == 0", OptimizeLevel::Advisory),
            Some((rule_ids::P001, "@is_empty(f(.x))".to_string()))
        );
    }

    #[test]
    fn deprecated_rename_no_panic_on_unclosed_paren() {
        assert_eq!(
            suggest_deprecated_plugin_rename("!@uuid(", plugin_signatures(), OptimizeLevel::Safe),
            None
        );
    }
}
