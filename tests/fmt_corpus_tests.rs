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
/// The single documented exception: `fmt` drops an *empty* `EXTRACT` section
/// (a no-op that extracts nothing — see `format_gctf_chain`), so those are
/// excluded from the multiset on both sides. Dropping any *non-empty* section,
/// or any non-EXTRACT section (e.g. an empty RESPONSE, which means "expect an
/// empty response" and must be preserved), is still caught.
#[test]
fn fmt_preserves_the_section_type_multiset_across_examples_corpus() {
    use grpctestify::parser::ast::{SectionContent, SectionType};
    fn type_multiset(doc: &grpctestify::parser::GctfDocument) -> Vec<Vec<String>> {
        doc.iter_chain()
            .map(|d| {
                let mut types: Vec<String> = d
                    .sections
                    .iter()
                    .filter(|s| {
                        !(matches!(s.section_type, SectionType::Extract)
                            && matches!(s.content, SectionContent::Empty))
                    })
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
