#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code

//! Characterizes the optimizer through the path the CLI actually uses —
//! `format_gctf_content_with_level`, i.e. the document-based
//! `collect_assertion_optimizations` that `run`/`fmt` run. The sibling
//! `optimizer_characterization_tests.rs` snapshots `rewrite_assertion_
//! expression_fixed_point_with_level`, a DIFFERENT entry point that diverges
//! from this one (e.g. I003 nested-if folds here but not there). This is the
//! guard the optimizer's AST migration (openspec §2.2) must hold, because it
//! reflects what a user's assertions become at runtime.
//!
//! Regenerate after an *intended* behaviour change:
//! `UPDATE_GOLDEN=1 cargo test --test optimizer_document_path_tests`

use grpctestify::commands::fmt::format_gctf_content_with_level;
use grpctestify::optimizer::OptimizeLevel;

#[path = "support/mod.rs"]
mod support;

/// Same expressions as the fixed-point corpus, plus the if/ternary shapes that
/// exposed the two paths diverging.
const CORPUS: &[&str] = &[
    "@is_uuid(.id) == true",
    "@is_uuid(.id) == false",
    "!!@is_uuid(.id)",
    "1 == 1",
    ".a == .a",
    ".a != .a",
    ".a or true",
    ".a or false",
    ".a and true",
    ".a and false",
    ".a or .b and false",
    ".a and (.b or false)",
    "if true then .a else .b end",
    "if false then .a else .b end",
    "if true then \"yes\" else \"no\" end",
    "if .c then .x else .x end",
    "if .c then (if .c then .a else .b end) else .z end",
    "if .c then true else false end",
    "if .c then false else true end",
    ".c ? .a : .b",
    ".name startswith(\"a\")",
    "!(.x == 1)",
    "!(.x == 1 and .y == 2)",
    "not (.x == 1 or .y == 2)",
    "@len(.x) == 0",
    "@len(.x) != 0",
    "!@is_empty(.a)",
    "@is_empty(.a) == false",
    "@is_empty(.a) and @is_empty(.b) == false",
    "@len(.x) >= 0",
    ".x:number == 1",
    ".msg == \" and \"",
    ".a // .b",
    "@is_empty(.a) and .b",
];

/// The single ASSERTS line after formatting `expr` at `level` — this is the
/// assertion the runtime would actually evaluate.
fn optimized_assert(expr: &str, level: OptimizeLevel) -> String {
    let src = format!(
        "--- ENDPOINT ---\ns.S/M\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{{}}\n\n--- ASSERTS ---\n{expr}\n"
    );
    let out = format_gctf_content_with_level(&src, "t.gctf", level)
        .unwrap_or_else(|e| panic!("format failed for {expr:?} at {level:?}: {e}"));
    // The ASSERTS body is the last section; take the line after the header.
    let body = out.rsplit("--- ASSERTS ---").next().unwrap();
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

fn render() -> String {
    let mut out = String::new();
    for level in [
        OptimizeLevel::Safe,
        OptimizeLevel::Advisory,
        OptimizeLevel::Aggressive,
    ] {
        out.push_str(&format!("=== {level:?} ===\n"));
        for expr in CORPUS {
            let got = optimized_assert(expr, level);
            let marker = if got == *expr { "  " } else { "->" };
            out.push_str(&format!("{marker} {expr}\n     {got}\n"));
        }
    }
    out
}

#[test]
fn optimizer_document_path_rewrites_are_unchanged() {
    support::assert_golden("optimizer_document_path.golden", &render());
}
