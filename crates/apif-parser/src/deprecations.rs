//! Single source of truth for detecting deprecated `.gctf` spellings.
//!
//! Runs over the shared `gctf_tokenizer` token stream (the same one both the
//! strict `core.rs` and lenient `error_recovery.rs` parsers consume), so every
//! command surfaces the *same* deprecation warnings from one place instead of
//! the scattered, per-command scans that used to drift (`check` via the
//! validator, the lenient path via its own inline check, LSP via yet another).
//!
//! Detection only — it does not mutate tokens. Canonicalization already happens
//! downstream (`SectionType::from_keyword` maps `HEADERS`→`REQUEST_HEADERS`, the
//! validator accepts kebab aliases, `fmt` re-emits canonical spellings), so the
//! missing piece was uniform *reporting*, not the rewrite.

use crate::ast::{DEPRECATED_KEBAB_CASE_KEYS, SectionType};
use crate::gctf_tokenizer::{GctfToken, GctfTokenKind, tokenize_kv_line};
use apif_diagnostics::{Diagnostic, DiagnosticCode, Range};

/// Canonical spelling for a deprecated kebab-case key, or `None` if `key` isn't
/// a known deprecated form.
fn deprecated_kebab_canonical(key: &str) -> Option<&'static str> {
    DEPRECATED_KEBAB_CASE_KEYS
        .iter()
        .find(|(deprecated, _)| *deprecated == key)
        .map(|(_, canonical)| *canonical)
}

/// Detect every deprecated spelling in the token stream, each as a `Warning`
/// diagnostic located at the offending line. Covers the three token-visible
/// forms: the `HEADERS` section alias, kebab-case `OPTIONS` keys, and kebab-case
/// `#[...]` attributes. (Deprecated *plugin* names live inside ASSERTS
/// expression text and stay with `apif_semantics::collect_deprecated_plugin_calls`,
/// which already is a single shared source.)
pub fn detect_deprecations(tokens: &[GctfToken]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut current_section: Option<SectionType> = None;

    for token in tokens {
        match &token.kind {
            GctfTokenKind::SectionHeader { name, .. } => {
                if name == "HEADERS" {
                    out.push(Diagnostic::warning(
                        DiagnosticCode::DeprecatedSymbol,
                        "HEADERS is deprecated, use REQUEST_HEADERS".to_string(),
                        Range::at_line(token.line),
                    ));
                }
                current_section = SectionType::from_keyword(name);
            }
            GctfTokenKind::AttributeBlock(attr_content) => {
                if let Some(attr) = crate::content_parser::parse_attribute(attr_content)
                    && let Some(canonical) = deprecated_kebab_canonical(&attr.name)
                {
                    out.push(Diagnostic::warning(
                        DiagnosticCode::DeprecatedSymbol,
                        format!(
                            "Attribute #[{}] is deprecated; prefer #[{}]",
                            attr.name, canonical
                        ),
                        Range::at_line(token.line),
                    ));
                }
            }
            GctfTokenKind::Content(text) => {
                // Kebab OPTIONS keys — only inside an OPTIONS section, so a
                // `retry-delay:`-shaped line in some other section's body (or a
                // JSON payload) can't false-match.
                if current_section == Some(SectionType::Options)
                    && let Some((key, _)) = tokenize_kv_line(text)
                    && let Some(canonical) = deprecated_kebab_canonical(&key)
                {
                    out.push(Diagnostic::warning(
                        DiagnosticCode::DeprecatedSymbol,
                        format!("OPTIONS.{key} is deprecated; prefer OPTIONS.{canonical}"),
                        Range::at_line(token.line),
                    ));
                }
            }
            GctfTokenKind::Comment(_) | GctfTokenKind::Blank => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gctf_tokenizer::tokenize_gctf;

    fn messages(source: &str) -> Vec<String> {
        detect_deprecations(&tokenize_gctf(source))
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn detects_headers_alias() {
        let msgs = messages("--- HEADERS ---\nx: 1\n");
        assert_eq!(msgs, ["HEADERS is deprecated, use REQUEST_HEADERS"]);
    }

    #[test]
    fn detects_kebab_options_keys() {
        let msgs = messages("--- OPTIONS ---\nretry-delay: 0.5\nno-retry: true\n");
        assert!(msgs.contains(
            &"OPTIONS.retry-delay is deprecated; prefer OPTIONS.retry_delay".to_string()
        ));
        assert!(
            msgs.contains(&"OPTIONS.no-retry is deprecated; prefer OPTIONS.no_retry".to_string())
        );
    }

    #[test]
    fn detects_kebab_attributes() {
        let msgs = messages("#[retry-delay(0.5)]\n#[no-retry]\n--- REQUEST ---\n{}\n");
        assert!(msgs.contains(
            &"Attribute #[retry-delay] is deprecated; prefer #[retry_delay]".to_string()
        ));
        assert!(
            msgs.contains(&"Attribute #[no-retry] is deprecated; prefer #[no_retry]".to_string())
        );
    }

    #[test]
    fn canonical_spellings_are_not_flagged() {
        let msgs = messages(
            "--- REQUEST_HEADERS ---\nx: 1\n\n--- OPTIONS ---\nretry_delay: 0.5\nno_retry: true\n",
        );
        assert!(msgs.is_empty(), "unexpected: {msgs:?}");
    }

    #[test]
    fn kebab_key_outside_options_is_not_flagged() {
        // A `retry-delay:`-shaped line in a REQUEST JSON body must not be
        // mistaken for a deprecated OPTIONS key.
        let msgs = messages("--- REQUEST ---\n{\n  \"retry-delay\": 5\n}\n");
        assert!(msgs.is_empty(), "unexpected: {msgs:?}");
    }

    #[test]
    fn reports_the_exact_offending_line() {
        let src = "--- OPTIONS ---\ntimeout: 5\nretry-delay: 0.5\n";
        let diags = detect_deprecations(&tokenize_gctf(src));
        assert_eq!(diags.len(), 1);
        // `retry-delay` is on line index 2 (0-based).
        assert_eq!(diags[0].range.start.line, 2);
    }
}
