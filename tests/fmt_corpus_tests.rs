#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
use grpctestify::commands::fmt::{format_gctf_content, format_gctf_content_with_level};
use grpctestify::optimizer::OptimizeLevel;
use grpctestify::parser::{parse_gctf_from_str, validate_document_chain};

fn corpus_files() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "gctf") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut out = Vec::new();
    walk(&root, &mut out);
    out.sort();
    out
}

/// `fmt(fmt(x)) == fmt(x)` across every real `.gctf` example in the repo.
#[test]
fn fmt_is_idempotent_across_examples_corpus() {
    let files = corpus_files();
    assert!(!files.is_empty(), "corpus must not be empty");
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let name = path.to_string_lossy();
        let once = format_gctf_content(&source, &name)
            .unwrap_or_else(|e| panic!("{name}: format failed: {e}"));
        let twice = format_gctf_content(&once, &name)
            .unwrap_or_else(|e| panic!("{name}: second format failed: {e}"));
        assert_eq!(once, twice, "{name}: fmt is not idempotent");
    }
}

/// `parse(x)` succeeds and validates ⟹ `parse(fmt(x))` also succeeds and
/// validates, across every real `.gctf` example — formatting must not change
/// what the file means.
#[test]
fn fmt_preserves_parseability_and_validity_across_examples_corpus() {
    let files = corpus_files();
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let name = path.to_string_lossy();

        let before = parse_gctf_from_str(&source, &name);
        let Ok(before_doc) = before else {
            continue; // not every example is meant to stand alone as valid GCTF
        };
        if validate_document_chain(&before_doc).is_err() {
            continue;
        }

        let formatted = format_gctf_content(&source, &name)
            .unwrap_or_else(|e| panic!("{name}: format failed: {e}"));
        let after_doc = parse_gctf_from_str(&formatted, &name)
            .unwrap_or_else(|e| panic!("{name}: formatted output no longer parses: {e}"));
        validate_document_chain(&after_doc)
            .unwrap_or_else(|e| panic!("{name}: formatted output no longer validates: {e:?}"));
    }
}

/// `fmt` must never add, drop, or change the *type* of a section — the
/// multiset of section types (per document in a chain, and the chain length
/// itself) is invariant under formatting, even though line numbers, spans,
/// section order, JSON key formatting, and deprecated-form canonicalization
/// all legitimately change. This is the safely-automatable core of
/// `parse(x) ≡ parse(fmt(x))`: a full structural-equivalence assertion isn't
/// clean to express (fmt legitimately rewrites content — kebab→snake keys,
/// JSON reflow, section reordering), but silent section loss/duplication —
/// the scariest formatting bug — is caught here.
///
/// No exceptions: `fmt` used to drop an *empty* `EXTRACT` section, excluded
/// here as a documented no-op. It was not a no-op — `inspect`, `explain` and
/// `Workflow::extractions` all report the section, so deleting it changed what
/// three tests observed. A section the author wrote is never fmt's to remove.
#[test]
fn fmt_preserves_the_section_type_multiset_across_examples_corpus() {
    fn type_multiset(doc: &grpctestify::parser::GctfDocument) -> Vec<Vec<String>> {
        doc.iter_chain()
            .map(|d| {
                let mut types: Vec<String> = d
                    .sections
                    .iter()
                    .map(|s| s.section_type.as_str().to_string())
                    .collect();
                types.sort();
                types
            })
            .collect()
    }

    for path in corpus_files() {
        let source = std::fs::read_to_string(&path).unwrap();
        let name = path.to_string_lossy();

        let Ok(before_doc) = parse_gctf_from_str(&source, &name) else {
            continue;
        };
        if validate_document_chain(&before_doc).is_err() {
            continue;
        }

        let formatted = format_gctf_content(&source, &name)
            .unwrap_or_else(|e| panic!("{name}: format failed: {e}"));
        let after_doc = parse_gctf_from_str(&formatted, &name)
            .unwrap_or_else(|e| panic!("{name}: formatted output no longer parses: {e}"));

        assert_eq!(
            type_multiset(&before_doc),
            type_multiset(&after_doc),
            "{name}: fmt changed the section-type multiset (added/dropped/retyped a section)"
        );
    }
}

/// Same two properties (idempotent, parse/validity-preserving) at every
/// optimizer level — `fmt`'s Advisory/Aggressive rewrites must be just as
/// safe on real files as the default Safe level, not just non-crashing.
#[test]
fn fmt_is_idempotent_and_valid_at_every_optimizer_level_across_examples_corpus() {
    let files = corpus_files();
    for level in [
        OptimizeLevel::None,
        OptimizeLevel::Safe,
        OptimizeLevel::Advisory,
        OptimizeLevel::Aggressive,
    ] {
        for path in &files {
            let source = std::fs::read_to_string(path).unwrap();
            let name = format!("{} [{level:?}]", path.to_string_lossy());

            let before = parse_gctf_from_str(&source, &name);
            let Ok(before_doc) = before else {
                continue;
            };
            if validate_document_chain(&before_doc).is_err() {
                continue;
            }

            let once = format_gctf_content_with_level(&source, &name, level)
                .unwrap_or_else(|e| panic!("{name}: format failed: {e}"));
            let twice = format_gctf_content_with_level(&once, &name, level)
                .unwrap_or_else(|e| panic!("{name}: second format failed: {e}"));
            assert_eq!(once, twice, "{name}: fmt is not idempotent");

            let after_doc = parse_gctf_from_str(&once, &name)
                .unwrap_or_else(|e| panic!("{name}: formatted output no longer parses: {e}"));
            validate_document_chain(&after_doc)
                .unwrap_or_else(|e| panic!("{name}: formatted output no longer validates: {e:?}"));
        }
    }
}

/// Every `//`/`#` comment payload in the source survives formatting, across
/// every real `.gctf` example. The corpus tests above pin structure and
/// validity; nothing pinned the author's prose, which is exactly what a
/// formatter is most likely to quietly drop.
#[test]
fn fmt_never_loses_comment_text_across_examples_corpus() {
    for path in corpus_files() {
        let source = std::fs::read_to_string(&path).unwrap();
        let name = path.to_string_lossy();
        let formatted = format_gctf_content(&source, &name).unwrap();
        for payload in comment_payloads(&source) {
            assert!(
                formatted.contains(&payload),
                "{name}: lost comment text {payload:?}"
            );
        }
    }
}

/// Comment bodies worth asserting on: long enough to be unambiguous, and
/// trimmed so the `#` -> `//` rewrite doesn't count as a difference.
fn comment_payloads(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let body = trimmed
            .strip_prefix("//")
            .or_else(|| trimmed.strip_prefix('#'))
            .map(str::trim);
        if let Some(body) = body
            && body.len() > 12
        {
            out.push(body.to_string());
        }
    }
    out
}

const COMMENT_PROBE_BODY: &str = r#"{
  "a": 1,
  "b": [1, 2],
  "c": {"d": "x"},
  "e": []
}"#;

/// Drop a comment at every structural position in a JSON body and assert the
/// three properties the formatter owes it: the file still formats, formatting
/// is idempotent, and a comment written at the end of a line of content stays
/// at the end of that line. The last one is the regression that motivated
/// this: a comment after a comma used to be pushed onto the following line,
/// silently re-attaching it to the next element.
#[test]
fn fmt_keeps_a_comment_where_it_was_written_at_every_position() {
    for (index, source) in probe_variants().into_iter().enumerate() {
        let name = format!("probe{index}.gctf");
        let marker = format!("mark{index}");
        let trailing = source
            .lines()
            .find(|l| l.contains(&marker))
            .map(|l| !l.trim_start().starts_with("//"))
            .unwrap();

        let once = format_gctf_content(&source, &name)
            .unwrap_or_else(|e| panic!("{name}: format failed: {e}\n--- source ---\n{source}"));
        let twice = format_gctf_content(&once, &name).unwrap();
        assert_eq!(once, twice, "{name}: not idempotent\n{once}");

        let line = once
            .lines()
            .find(|l| l.contains(&marker))
            .unwrap_or_else(|| panic!("{name}: comment vanished\n{once}"));
        assert_eq!(
            trailing,
            !line.trim_start().starts_with("//"),
            "{name}: comment moved off its line\n--- source ---\n{source}\n--- got ---\n{once}"
        );
    }
}

/// One variant per structural position: `// markN` inserted after each `,`,
/// `{`, `[` and at each end of line, outside string literals.
fn probe_variants() -> Vec<String> {
    let mut positions: Vec<usize> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let bytes = COMMENT_PROBE_BODY.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if in_string && escaped {
            escaped = false;
            continue;
        }
        if in_string && b == b'\\' {
            escaped = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if matches!(b, b',' | b'{' | b'[') || b == b'\n' {
            positions.push(if b == b'\n' { i } else { i + 1 });
        }
    }
    positions.dedup();

    positions
        .into_iter()
        .enumerate()
        .map(|(index, at)| {
            let mut body = COMMENT_PROBE_BODY.to_string();
            // A `//` comment owns the rest of its line, so anything that
            // followed the insertion point continues on the next one — which
            // is how an author would have written it by hand anyway.
            let tail = if body[at..].starts_with('\n') { "" } else { "\n" };
            body.insert_str(at, &format!(" // mark{index}{tail}"));
            format!(
                "--- ENDPOINT ---\ntest.Service/Method\n\n--- REQUEST ---\n{{}}\n\n--- RESPONSE ---\n{body}\n"
            )
        })
        .collect()
}
