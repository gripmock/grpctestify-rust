#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! Pins what the assertion optimizer actually does, expression by expression,
//! at every level — before any refactor touches it.
//!
//! Every rule here is implemented by string surgery (`find(" and ")`,
//! `strip_prefix("@is_empty(")`, manual paren counting) rather than over the
//! assertion AST that `apif-ast` already provides. Moving them onto the AST is
//! the obvious simplification, and it is also the change most likely to alter
//! behaviour silently: three of these rules were rewriting assertions into
//! something that meant something else, two of them at Safe level, i.e. inside
//! `run`. So the behaviour gets frozen first and the refactor has to reproduce
//! this table exactly.
//!
//! Regenerate after an *intended* behaviour change:
//! `UPDATE_GOLDEN=1 cargo test --test optimizer_characterization_tests`

use apif_optimizer::{OptimizeLevel, rewrite_assertion_expression_fixed_point_with_level};

#[path = "support/mod.rs"]
mod support;

/// Expressions covering every rule id, plus the shapes that must never be
/// rewritten (a boolean operator inside a string, jq's `//` alternative,
/// a compound expression a single-comparison rule must decline).
const CORPUS: &[&str] = &[
    "@is_uuid(.id) == true",
    "@is_uuid(.id) == false",
    "true == @is_uuid(.id)",
    "false == @is_uuid(.id)",
    "@is_uuid(.id) != true",
    "@is_uuid(.id) != false",
    "true != @is_uuid(.id)",
    "false != @is_uuid(.id)",
    "!!@is_uuid(.id)",
    "not not @is_uuid(.id)",
    "1 == 1",
    "\"a\" == \"b\"",
    ".a == .a",
    ".a != .a",
    ".a or true",
    ".a or false",
    ".a and true",
    ".a and false",
    ".a or .b and false",
    ".a and (.b or false)",
    "if true then .a else .b end",
    "if .c then .x else .x end",
    "if .c then true else false end",
    "if .c then false else true end",
    "if true then \"yes\" else \"no\" end",
    "if false then \"yes\" else \"no\" end",
    "if true then .a else .b end",
    "if false then .a else .b end",
    "if .c then \"x\" else \"x\" end",
    "if .c then (if .c then .a else .b end) else .z end",
    "if .c then true else false end",
    ".c ? .a : .b",
    ".c ? true : false",
    "if (.x == 1) then .a else .b end",
    ".name startswith(\"a\")",
    ".a | endswith(\"x\")",
    "!(.x == 1)",
    "!(.x == 1 and .y == 2)",
    "not (.x == 1 or .y == 2)",
    "@len(.x) == 0",
    "@len(.x) != 0",
    "(.a)",
    "!@is_empty(.a)",
    "@is_empty(.a) == false",
    "false == @is_empty(.a)",
    "@is_empty(.a) and @is_empty(.b) == false",
    "!@is_empty(f(.a, g(.b)))",
    "@len(.x) >= 0",
    ".x:number == 1",
    ".msg == \" and \"",
    ".a // .b",
    "@is_empty(.a) and .b",
    ".x == 1 // note",
];

fn render() -> String {
    let mut out = String::new();
    for level in [
        OptimizeLevel::Layout,
        OptimizeLevel::Safe,
        OptimizeLevel::Advisory,
        OptimizeLevel::Aggressive,
    ] {
        out.push_str(&format!("=== {level:?} ===\n"));
        for expr in CORPUS {
            let got = rewrite_assertion_expression_fixed_point_with_level(expr, level);
            let marker = if got == *expr { "  " } else { "->" };
            out.push_str(&format!("{marker} {expr}\n     {got}\n"));
        }
    }
    out
}

#[test]
fn optimizer_rewrites_are_unchanged() {
    support::assert_golden("optimizer_rewrites.golden", &render());
}
