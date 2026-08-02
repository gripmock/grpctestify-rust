#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `parse_assertion` and `assertion_to_string` are an encode/decode pair, and
//! anything that rewrites an assertion (the optimizer's AST rules, `fmt`'s
//! canonicaliser) round-trips through them. When the pair is not faithful, the
//! rewrite silently changes what a test asserts — which is how `fmt --write`
//! came to turn `"a\\b"` into `"ab"`.
//!
//! Two properties, over every assertion in the repo's own fixtures plus the
//! shapes that broke before:
//!
//! - re-rendering is stable (`render(parse(render(parse(x)))) == render(parse(x))`)
//! - the render never invents or drops a string literal's contents

use grpctestify::parser::assertion_ast::{AssertionExpr, assertion_to_string, parse_assertion};

/// Every ASSERTS line in the fixture corpus, plus literals known to be lossy.
fn assertions() -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "gctf") {
                out.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk(&root.join("examples"), &mut files);
    walk(&root.join("tests/gctf"), &mut files);
    files.sort();

    let mut out: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut in_asserts = false;
        for line in text.lines() {
            if line.starts_with("--- ") {
                in_asserts = line.starts_with("--- ASSERTS");
                continue;
            }
            let trimmed = line.trim();
            if !in_asserts
                || trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("//")
            {
                continue;
            }
            out.push(trimmed.to_string());
        }
    }

    out.extend(
        [
            r#"!(.p == "a\\b")"#,
            r#"!(.q == "tab\there")"#,
            r#".m == "say \"hi\"""#,
            r#".path == "C:\\tmp""#,
            r#".plain == "abc""#,
            ".a > 1 and .b < 2",
            "@len(.items) > 0",
        ]
        .map(String::from),
    );
    out
}

#[test]
fn rendering_an_assertion_is_stable() {
    let mut unstable = Vec::new();
    let mut checked = 0usize;

    for expr in assertions() {
        let ast = parse_assertion(&expr);
        if matches!(ast, AssertionExpr::Raw(_)) {
            continue;
        }
        let once = assertion_to_string(&ast);
        let reparsed = parse_assertion(&once);
        if matches!(reparsed, AssertionExpr::Raw(_)) {
            // Known asymmetry: `if..then..else..end` renders as a ternary the
            // parser does not accept back. `fmt` guards against it explicitly.
            continue;
        }
        checked += 1;
        let twice = assertion_to_string(&reparsed);
        if once != twice {
            unstable.push(format!("{expr}\n  once:  {once}\n  twice: {twice}"));
        }
    }

    assert!(checked > 0, "no assertions were checked");
    assert!(
        unstable.is_empty(),
        "{} assertions render differently on a second pass:\n{}",
        unstable.len(),
        unstable.join("\n")
    );
}

#[test]
fn rendering_never_changes_a_string_literal() {
    let mut lossy = Vec::new();

    for expr in assertions() {
        let ast = parse_assertion(&expr);
        if matches!(ast, AssertionExpr::Raw(_)) {
            continue;
        }
        let rendered = assertion_to_string(&ast);

        // Compare only the string-literal payloads: layout differences (spacing,
        // parens) are what canonicalisation is *for*.
        let original_literals = string_literals(&expr);
        let rendered_literals = string_literals(&rendered);

        if original_literals != rendered_literals {
            lossy.push(format!(
                "{expr}\n  literals before: {original_literals:?}\n  literals after:  {rendered_literals:?}"
            ));
        }
    }

    assert!(
        lossy.is_empty(),
        "{} assertions lose literal contents when re-rendered:\n{}",
        lossy.len(),
        lossy.join("\n")
    );
}

/// Raw string-literal payloads, in order, without interpreting escapes.
fn string_literals(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' && c != '\'' {
            continue;
        }
        let quote = c;
        let mut buf = String::new();
        while let Some(n) = chars.next() {
            if n == '\\' {
                buf.push(n);
                if let Some(escaped) = chars.next() {
                    buf.push(escaped);
                }
                continue;
            }
            if n == quote {
                break;
            }
            buf.push(n);
        }
        out.push(buf);
    }
    out
}
