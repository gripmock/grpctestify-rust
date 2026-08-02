#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! The repo has two parsers for one format: a strict one (`check`, `fmt`) and a
//! recovering one (`run`, `bench`, `call`, `explain`, `grpcurl`, `index`, LSP).
//! Four separate bugs came from them quietly disagreeing — BENCH `sources` was
//! dropped, JsonLines streaming became `Empty`, DATASET errors were swallowed,
//! and EXTRACT stored the raw ternary instead of its jq form. Each was found by
//! hand, one at a time, after it had already shipped.
//!
//! This compares them over the whole fixture corpus instead: for every valid
//! `.gctf` in the repo, a section that the strict parser accepts must come out
//! of the recovering parser with the same content.

use grpctestify::parser::ast::{GctfDocument, SectionContent};
use grpctestify::parser::{parse_content_with_recovery, parse_gctf_from_str};

fn corpus() -> Vec<std::path::PathBuf> {
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
    // Opt-in extra corpus: `GCTF_EXTRA_CORPUS=/path/to/other/fixtures` widens
    // the comparison to a foreign fixture tree without committing to its path.
    if let Ok(extra) = std::env::var("GCTF_EXTRA_CORPUS") {
        walk(std::path::Path::new(&extra), &mut files);
    }
    files.sort();
    files
}

/// A label naming what the section holds, so a mismatch says which shape
/// differed rather than dumping two payloads.
fn shape(content: &SectionContent) -> &'static str {
    match content {
        SectionContent::Single(_) => "single",
        SectionContent::Json(_) => "json",
        SectionContent::JsonLines(_) => "json-lines",
        SectionContent::KeyValues(_) => "key-values",
        SectionContent::Extract(_) => "extract",
        SectionContent::Assertions(_) => "assertions",
        SectionContent::Meta(_) => "meta",
        SectionContent::Rows(_) => "rows",
        SectionContent::Empty => "empty",
    }
}

fn flatten(doc: &GctfDocument) -> Vec<(String, SectionContent)> {
    let mut out = Vec::new();
    let mut current = Some(doc);
    while let Some(d) = current {
        for section in &d.sections {
            out.push((
                format!("{:?}", section.section_type),
                section.content.clone(),
            ));
        }
        current = d.next_document.as_deref();
    }
    out
}

#[test]
fn both_parsers_agree_on_every_valid_fixture() {
    let files = corpus();
    assert!(!files.is_empty(), "corpus must not be empty");

    let mut mismatches = Vec::new();
    let mut compared = 0usize;

    for path in &files {
        let content = std::fs::read_to_string(path).expect("read fixture");
        let label = path.display().to_string();

        // Only files the strict parser accepts are in scope — the recovering
        // parser is meant to differ on malformed input, that is its job.
        let Ok(strict) = parse_gctf_from_str(&content, &label) else {
            continue;
        };
        let lenient = parse_content_with_recovery(&content, &label).document;

        let (a, b) = (flatten(&strict), flatten(&lenient));
        if a.len() != b.len() {
            mismatches.push(format!(
                "{label}: strict has {} sections, recovering has {}",
                a.len(),
                b.len()
            ));
            continue;
        }

        for (i, ((ty_a, ca), (ty_b, cb))) in a.iter().zip(b.iter()).enumerate() {
            compared += 1;
            if ty_a != ty_b {
                mismatches.push(format!(
                    "{label}: section {i} is {ty_a} strictly but {ty_b} recovering"
                ));
                continue;
            }
            if ca != cb {
                mismatches.push(format!(
                    "{label}: {ty_a} section {i} differs — strict {}, recovering {}",
                    shape(ca),
                    shape(cb)
                ));
            }
        }
    }

    assert!(compared > 0, "no sections were compared");
    assert!(
        mismatches.is_empty(),
        "the two parsers disagree on {} of {compared} sections:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
