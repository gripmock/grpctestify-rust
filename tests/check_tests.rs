#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! `check` command regression coverage.

#[path = "support/mod.rs"]
mod support;

/// `check` didn't surface dead EXTRACT variables at all — only the LSP did
/// (`crate::lsp::handlers::collect_unused_variables`, wired into editor
/// diagnostics but never called from `check`). A variable extracted but
/// never referenced via `{{var_name}}` in a later document was silently
/// invisible outside an editor.
#[test]
fn check_flags_unused_extract_variable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("unused.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- EXTRACT ---\nstatus_val = .status\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("UNUSED_VARIABLE") && stdout.contains("status_val"),
        "expected an UNUSED_VARIABLE warning for status_val: {stdout}"
    );
}

/// A variable that IS referenced via `{{var_name}}` in a later document must
/// not be flagged — the check is chain-aware, not just single-document.
#[test]
fn check_does_not_flag_used_extract_variable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("used.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"status\": \"SERVING\"}\n\n--- EXTRACT ---\nstatus_val = .status\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{\"note\": \"{{status_val}}\"}\n\n--- RESPONSE ---\n{\"status\": \"SERVING\"}\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("UNUSED_VARIABLE"),
        "status_val is used via {{{{status_val}}}} in doc 2, must not be flagged: {stdout}"
    );
}

/// A `literal == literal` assertion never actually checks the response —
/// almost always a typo (`.status == "SERVING"` fat-fingered into
/// `"SERVING" == "SERVING"`). No detection existed for this anywhere.
#[test]
fn check_flags_constant_assertion() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("constant.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n\"SERVING\" == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SEM_C001") && stdout.contains("always evaluates to true"),
        "expected a SEM_C001 constant-assertion warning: {stdout}"
    );
}

/// A real field comparison must never be flagged as constant.
#[test]
fn check_does_not_flag_field_comparison_as_constant() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("real.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("SEM_C001"),
        ".status == \"SERVING\" is a real check, must not be flagged: {stdout}"
    );
}

#[test]
fn check_flags_duplicate_assertion() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SEM_C002") && stdout.contains("Duplicate assertion"),
        "expected a SEM_C002 duplicate-assertion warning: {stdout}"
    );
}

#[test]
fn check_does_not_flag_distinct_assertions_as_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("distinct.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n.count > 0\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("SEM_C002"),
        "distinct assertions: {stdout}"
    );
}

#[test]
fn check_flags_suite_with_zero_asserts_coverage() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.gctf"),
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"status\": \"SERVING\"}\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &dir.path().to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("NO_ASSERTS_COVERAGE"),
        "expected a suite-wide NO_ASSERTS_COVERAGE warning: {stdout}"
    );
}

#[test]
fn check_does_not_flag_suite_with_asserts_coverage() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.gctf"),
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &dir.path().to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("NO_ASSERTS_COVERAGE"),
        "suite has an ASSERTS section, must not be flagged: {stdout}"
    );
}

/// `--optimize advisory` hints (e.g. `OPT_P001`, Advisory-only) must reach
/// `check`'s structured JSON output, not just its human-readable text —
/// and must NOT appear at the default (Safe) level.
#[test]
fn check_surfaces_advisory_optimizer_hints_in_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("advisory.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n@len(.tags) == 0\n",
    )
    .unwrap();

    let default_output = support::cli_command()
        .args(["check", "--format", "json", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let default_json = support::parse_json_stdout(&default_output);
    assert!(
        !default_json.to_string().contains("OPT_P001"),
        "OPT_P001 is Advisory-only, must not appear at the default Safe level: {default_json}"
    );

    let advisory_output = support::cli_command()
        .args([
            "check",
            "--optimize",
            "advisory",
            "--format",
            "json",
            &file.to_string_lossy(),
        ])
        .output()
        .expect("failed to run check");
    let advisory_json = support::parse_json_stdout(&advisory_output);
    assert!(
        advisory_json.to_string().contains("OPT_P001"),
        "expected OPT_P001 in --optimize advisory JSON output: {advisory_json}"
    );
}

/// `--- HEADERS ---` (deprecated alias of `--- REQUEST_HEADERS ---`) must be
/// flagged by `check` — this had zero regression coverage despite the
/// warning mechanism (`GctfDocument::section_uses_deprecated_headers_alias`)
/// already being wired into `check`/`inspect`/LSP.
#[test]
fn check_flags_deprecated_headers_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("headers.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- HEADERS ---\nauthorization: Bearer token\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", "--format", "json", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let json = support::parse_json_stdout(&output);
    assert!(
        json.to_string().contains("DEPRECATED_SECTION"),
        "expected a DEPRECATED_SECTION warning for HEADERS: {json}"
    );
}

/// `fmt` must canonicalize `--- HEADERS ---` to `--- REQUEST_HEADERS ---`,
/// and doing so a second time must be a no-op (idempotent).
#[test]
fn fmt_canonicalizes_deprecated_headers_section() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("headers.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- HEADERS ---\nauthorization: Bearer token\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let write_once = support::cli_command()
        .args(["fmt", "-w", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt -w");
    assert!(write_once.status.success(), "{write_once:?}");

    let formatted = std::fs::read_to_string(&file).unwrap();
    assert!(
        formatted.contains("--- REQUEST_HEADERS ---"),
        "expected HEADERS to be canonicalized to REQUEST_HEADERS: {formatted}"
    );
    assert!(!formatted.contains("--- HEADERS ---"));

    let check_after = support::cli_command()
        .args(["fmt", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt (check mode)");
    assert!(
        check_after.status.success(),
        "fmt is not idempotent after canonicalizing HEADERS: {:?}",
        String::from_utf8_lossy(&check_after.stdout)
    );
}

/// An ASSERTS `.field == literal` line whose exact value is already pinned
/// by a `RESPONSE with_asserts` body must be flagged as redundant.
#[test]
fn check_flags_redundant_response_assertion() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("redundant.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE with_asserts ---\n{\"status\": \"SERVING\"}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SEM_C003") && stdout.contains("already pinned"),
        "expected a SEM_C003 redundant-assertion warning: {stdout}"
    );
}

/// The same field asserted to a DIFFERENT value than RESPONSE pins must not
/// be flagged — that's a real (if self-contradictory) check, not redundant.
#[test]
fn check_does_not_flag_non_matching_response_assertion() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-redundant.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE with_asserts ---\n{\"status\": \"SERVING\"}\n\n--- ASSERTS ---\n.status != \"NOT_SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("SEM_C003"),
        "a `!=` comparison is not redundant with an exact-match pin: {stdout}"
    );
}

/// `fmt` must canonicalize legacy kebab-case OPTIONS keys
/// (`retry-delay`/`no-retry` → `retry_delay`/`no_retry`) and sort every
/// OPTIONS key alphabetically — this already worked
/// (`src/commands/fmt.rs::format_options_section`) but had no regression
/// test anywhere.
#[test]
fn fmt_canonicalizes_legacy_options_keys_and_order() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("options.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- OPTIONS ---\ntimeout: 30\nretry-delay: 1.5\nno-retry: false\nretry: 2\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let write_once = support::cli_command()
        .args(["fmt", "-w", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt -w");
    assert!(write_once.status.success(), "{write_once:?}");

    let formatted = std::fs::read_to_string(&file).unwrap();
    assert!(!formatted.contains("retry-delay"));
    assert!(!formatted.contains("no-retry"));
    assert!(formatted.contains("retry_delay: 1.5"));
    assert!(formatted.contains("no_retry: false"));

    // Alphabetical: no_retry, retry, retry_delay, timeout.
    let options_start = formatted.find("--- OPTIONS ---").unwrap();
    let no_retry_pos = formatted[options_start..].find("no_retry").unwrap();
    let retry_pos = formatted[options_start..].find("retry:").unwrap();
    let retry_delay_pos = formatted[options_start..].find("retry_delay").unwrap();
    let timeout_pos = formatted[options_start..].find("timeout").unwrap();
    assert!(no_retry_pos < retry_pos);
    assert!(retry_pos < retry_delay_pos);
    assert!(retry_delay_pos < timeout_pos);

    let check_after = support::cli_command()
        .args(["fmt", &file.to_string_lossy()])
        .output()
        .expect("failed to run fmt (check mode)");
    assert!(
        check_after.status.success(),
        "fmt is not idempotent after canonicalizing OPTIONS: {:?}",
        String::from_utf8_lossy(&check_after.stdout)
    );
}

/// A duplicate key in a KV section (OPTIONS/TLS/PROTO/REQUEST_HEADERS) used
/// to silently last-win — `check` must now hard-fail on it instead.
#[test]
fn check_fails_on_duplicate_options_key() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup-options.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- OPTIONS ---\ntimeout: 30\ntimeout: 60\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", "--format", "json", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    assert!(!output.status.success());
    let json = support::parse_json_stdout_any_status(&output);
    assert!(
        json.to_string().contains("duplicate key 'timeout'"),
        "expected a duplicate-key parse error: {json}"
    );
}

/// Same for a duplicate EXTRACT variable name.
#[test]
fn check_fails_on_duplicate_extract_variable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup-extract.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{\"status\": \"SERVING\"}\n\n--- EXTRACT ---\nstatus_val = .status\nstatus_val = .status\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", "--format", "json", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    assert!(!output.status.success());
    let json = support::parse_json_stdout_any_status(&output);
    assert!(
        json.to_string()
            .contains("duplicate EXTRACT variable 'status_val'"),
        "expected a duplicate-variable parse error: {json}"
    );
}

/// `check --format json` reports each document's section presence — the
/// JSON-only "structure completeness" view (text mode stays quiet, per its
/// existing convention of only printing when something needs attention).
#[test]
fn check_json_reports_per_document_structure() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("chain.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- RESPONSE ---\n{}\n\n--- ENDPOINT ---\ngrpc.health.v1.Health/Watch\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", "--format", "json", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let json = support::parse_json_stdout(&output);
    let structure = json["structure"].as_array().expect("structure array");
    assert_eq!(structure.len(), 2, "{json}");
    assert_eq!(structure[0]["document_index"], 1);
    assert_eq!(
        structure[0]["sections"],
        serde_json::json!(["ENDPOINT", "REQUEST", "RESPONSE"])
    );
    assert_eq!(structure[1]["document_index"], 2);
    assert_eq!(
        structure[1]["sections"],
        serde_json::json!(["ENDPOINT", "REQUEST", "ASSERTS"])
    );
}

/// Text-mode `check` output must stay unchanged by the new JSON-only
/// `structure` field — it doesn't print a structure table.
#[test]
fn check_text_mode_unaffected_by_structure_field() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\ngrpc.health.v1.Health/Check\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n.status == \"SERVING\"\n",
    )
    .unwrap();

    let output = support::cli_command()
        .args(["check", &file.to_string_lossy()])
        .output()
        .expect("failed to run check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("structure"), "{stdout}");
}

/// Every shipped example must pass `check`. The examples are documentation —
/// a reader copies them — but nothing verified they stay valid: the only
/// suite covering the whole directory is `fmt_corpus_tests`, and idempotent
/// formatting says nothing about whether the file is still a correct `.gctf`.
/// `check` needs no network, so this stays cheap.
///
/// Missing-ADDRESS warnings are expected and fine: examples deliberately leave
/// the address to `GRPCTESTIFY_ADDRESS` or `--address`.
#[test]
fn every_example_passes_check() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let output = support::cli_command()
        .args(["check", &examples.to_string_lossy()])
        .output()
        .expect("failed to run check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "examples/ must stay valid .gctf\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("PASSED"),
        "expected a PASSED summary, got:\n{stdout}"
    );
}
