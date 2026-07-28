#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! LSP awareness of convention-directory `.rhai` plugins — closes a real
//! gap: `get_assertion_completions`/`get_plugin_hover`/`get_plugin_signatures`
//! (signature help)/`infer_type_label` (inlay hints) all used to build a
//! bare `PluginManager::new()` (built-ins only), and
//! `collect_semantic_diagnostics` never registered the convention
//! directories with `apif_semantics`/`apif_optimizer` the way
//! `check`/`fmt`/`explain`/`inspect` do — so a valid custom plugin was
//! invisible to completion/hover and flagged as an "unknown plugin" error
//! in the editor despite `run` executing it fine.
//!
//! `apif_semantics`/`apif_optimizer`'s registries are `OnceLock`-backed
//! (set once per process) and plugin-dir resolution reads `$HOME`/
//! `GRPCTESTIFY_PROJECT_DIR` from the process environment — both are
//! process-global state, so this lives in its own test binary with exactly
//! one `#[test]` to avoid racing with any other test that touches the same
//! globals.

use grpctestify::lsp::handlers;
use grpctestify::parser;

#[test]
fn lsp_recognizes_a_convention_directory_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        dir.path().join(".grpctestify/plugins/is_even.rhai"),
        "/// Checks a value is even.\n/// @param value: number\n/// @returns bool\n/// @pure\nfn is_even(value) { value % 2 == 0 }",
    )
    .unwrap();

    // SAFETY: this test binary contains exactly one #[test], so no other
    // test can race on these process-global environment variables.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::set_var("GRPCTESTIFY_PROJECT_DIR", dir.path());
    }

    // Completion: the custom plugin must appear alongside built-ins.
    let completions = handlers::get_assertion_completions();
    assert!(
        completions.iter().any(|c| c.label == "@is_even(...)"),
        "custom plugin missing from completions: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );

    // Hover: must resolve the custom plugin's doc-comment description.
    let content = "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n@is_even(.count)\n";
    let doc = parser::parse_gctf_from_str(content, "test.gctf").unwrap();
    let line_idx = content
        .lines()
        .position(|l| l.contains("@is_even"))
        .unwrap();
    let hover = handlers::get_plugin_hover(&doc, line_idx, 1);
    assert!(
        hover.is_some(),
        "hover must resolve a convention-dir plugin"
    );

    // Diagnostics: registering the convention dir's plugin names (the same
    // call `initialize()` makes) must suppress the false-positive
    // "unknown plugin" error for @is_even...
    apif_semantics::register_extra_plugin_names(grpctestify::commands::check::rhai_plugin_names());
    apif_optimizer::register_extra_boolean_plugins(
        grpctestify::commands::check::rhai_boolean_plugin_names(),
    );
    let diagnostics = handlers::collect_semantic_diagnostics(&doc, content);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("is_even") && d.message.to_lowercase().contains("unknown")),
        "a registered convention-dir plugin must not be flagged unknown: {diagnostics:?}"
    );

    // ...but a genuinely unregistered name still must be.
    let bad_content =
        "--- ENDPOINT ---\ntest.Service/Method\n\n--- ASSERTS ---\n@totally_made_up(.count)\n";
    let bad_doc = parser::parse_gctf_from_str(bad_content, "test.gctf").unwrap();
    let bad_diagnostics = handlers::collect_semantic_diagnostics(&bad_doc, bad_content);
    assert!(
        bad_diagnostics
            .iter()
            .any(|d| d.message.to_lowercase().contains("unknown")),
        "a genuinely unknown plugin must still be flagged: {bad_diagnostics:?}"
    );
}
