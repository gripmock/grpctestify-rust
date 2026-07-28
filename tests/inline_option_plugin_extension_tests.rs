#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! A `.rhai` plugin can declare a custom inline-option key via the bare
//! `@inline_option` doc tag, e.g. `--- RESPONSE my_check=5 ---` — the parser
//! otherwise hard-rejects any inline-option key it doesn't recognize (see
//! `crates/apif-parser/src/content_parser.rs::parse_inline_options`).
//!
//! `apif_parser`'s `EXTRA_INLINE_OPTION_KEYS` registry is `OnceLock`-backed
//! (set once per process) and plugin-dir resolution reads `$HOME`/
//! `GRPCTESTIFY_PROJECT_DIR` from the process environment — both are
//! process-global state, so this lives in its own test binary with exactly
//! one `#[test]`, same convention `tests/lsp_plugin_dir_tests.rs` uses.

use grpctestify::parser;

#[test]
fn plugin_declared_inline_option_key_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        dir.path().join(".grpctestify/plugins/priority.rhai"),
        "/// Marks a section as high priority for a custom reporter.\n/// @inline_option\nfn priority(value) { true }",
    )
    .unwrap();

    // SAFETY: this test binary contains exactly one #[test], so no other
    // test can race on these process-global environment variables.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::set_var("GRPCTESTIFY_PROJECT_DIR", dir.path());
    }

    let keys = grpctestify::plugins::rhai_plugin::load_all_inline_option_keys();
    assert!(
        keys.contains("priority"),
        "expected `priority` in plugin-declared inline-option keys: {keys:?}"
    );
    parser::register_extra_inline_option_keys(keys);

    // A key no plugin has declared is still a hard parse error.
    let unregistered =
        "--- ENDPOINT ---\ntest.Service/Method\n\n--- RESPONSE severity=high ---\n{}\n";
    assert!(
        parser::parse_gctf_from_str(unregistered, "test.gctf").is_err(),
        "a genuinely unregistered inline-option key must still be rejected"
    );

    // The plugin-declared key is accepted and its raw value captured.
    let registered =
        "--- ENDPOINT ---\ntest.Service/Method\n\n--- RESPONSE priority=urgent ---\n{}\n";
    let doc = parser::parse_gctf_from_str(registered, "test.gctf")
        .expect("a plugin-declared inline-option key must parse");
    let response = doc
        .sections
        .iter()
        .find(|s| s.section_type == parser::ast::SectionType::Response)
        .expect("RESPONSE section");
    assert_eq!(
        response.inline_options.extra.get("priority"),
        Some(&"urgent".to_string())
    );
}
