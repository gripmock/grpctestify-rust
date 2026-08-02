#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

#[path = "support/mod.rs"]
mod support;
use support::run_cli;

#[test]
fn docs_generates_index_and_service_page() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("get-user.gctf");
    let content = r#"--- META ---
name: Get a user
summary: Fetches a user by id.
tags: [smoke]
owner: team-users

--- ENDPOINT ---
users.UserService/GetUser

--- REQUEST ---
{"id": 1}

--- RESPONSE ---
{"id": 1, "name": "Ada"}

--- ASSERTS ---
.name == "Ada"
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let out_dir = dir.path().join("docs-out");
    let output = run_cli(&[
        "docs",
        &dir.path().to_string_lossy(),
        "--output",
        &out_dir.to_string_lossy(),
    ]);
    assert!(
        output.status.success(),
        "docs command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let index = std::fs::read_to_string(out_dir.join("index.md")).expect("index.md written");
    assert!(index.contains("[users.UserService](users.UserService.md)"));

    let page = std::fs::read_to_string(out_dir.join("users.UserService.md"))
        .expect("service page written");
    assert!(page.contains("## Get a user"));
    assert!(page.contains("Fetches a user by id."));
    assert!(page.contains("`smoke`"));
    assert!(page.contains("team-users"));
    assert!(page.contains("**Endpoint:** `users.UserService/GetUser`"));
    assert!(page.contains(r#""id": 1"#));
    assert!(page.contains(r#""name": "Ada""#));
    assert!(page.contains(r#"`.name == "Ada"`"#));
    // A single-document test has nothing to sequence — no mermaid diagram noise.
    assert!(!page.contains("```mermaid"));
}

#[test]
fn docs_renders_mermaid_diagram_for_multi_document_chain() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("chain.gctf");
    let content = r#"--- ENDPOINT ---
shop.CartService/AddItem

--- REQUEST ---
{"item_id": "x"}

--- RESPONSE ---
{"ok": true}

--- ENDPOINT ---
shop.CartService/Checkout

--- REQUEST ---
{}

--- RESPONSE ---
{"order_id": "o-1"}
"#;
    std::fs::write(&file, content).expect("failed to write temp gctf file");

    let out_dir = dir.path().join("docs-out");
    let output = run_cli(&[
        "docs",
        &dir.path().to_string_lossy(),
        "--output",
        &out_dir.to_string_lossy(),
    ]);
    assert!(
        output.status.success(),
        "docs command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let page =
        std::fs::read_to_string(out_dir.join("shop.CartService.md")).expect("service page written");
    assert!(page.contains("```mermaid"));
    assert!(page.contains("Client->>Server: AddItem"));
    assert!(page.contains("Client->>Server: Checkout"));
    assert!(page.contains("### Step 1"));
    assert!(page.contains("### Step 2"));
}

#[test]
fn docs_embeds_coverage_badge_when_report_given() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("get-user.gctf");
    std::fs::write(
        &file,
        r#"--- ENDPOINT ---
users.UserService/GetUser

--- REQUEST ---
{}

--- RESPONSE ---
{}
"#,
    )
    .unwrap();

    let coverage_path = dir.path().join("coverage.json");
    std::fs::write(
        &coverage_path,
        r#"{
  "files": [{"uri": "grpc://UserService", "statements": {"covered": 1, "total": 4}, "methods": []}],
  "messages": [],
  "summary": {"covered": 1, "total": 4},
  "field_summary": {"covered": 0, "total": 0}
}"#,
    )
    .unwrap();

    let out_dir = dir.path().join("docs-out");
    let output = run_cli(&[
        "docs",
        &dir.path().to_string_lossy(),
        "--output",
        &out_dir.to_string_lossy(),
        "--coverage",
        &coverage_path.to_string_lossy(),
    ]);
    assert!(
        output.status.success(),
        "docs command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let index = std::fs::read_to_string(out_dir.join("index.md")).unwrap();
    assert!(index.contains("**Overall coverage:** 1/4 methods called (25.0%)"));

    let page = std::fs::read_to_string(out_dir.join("users.UserService.md")).unwrap();
    assert!(
        page.contains("**Coverage:** 1/4 methods called (25.0%)"),
        "packaged service must match coverage by its bare service name: {page}"
    );
}

#[test]
fn docs_on_directory_with_no_endpoint_tests_reports_nothing_to_document() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("no-endpoint.gctf");
    // No ENDPOINT section — e.g. a helper/teardown file — must not crash or
    // produce an empty/garbage service page.
    std::fs::write(&file, "--- META ---\nname: helper\n").unwrap();

    let out_dir = dir.path().join("docs-out");
    let output = run_cli(&[
        "docs",
        &dir.path().to_string_lossy(),
        "--output",
        &out_dir.to_string_lossy(),
    ]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("nothing to document"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!out_dir.exists(), "no output directory should be created");
}

/// Every ```gctf block in the docs must at least parse.
///
/// The docs carry 62 of these and a reader copies them verbatim, but nothing
/// checked them: `docs_tests` covers the `docs` *command*, and
/// `fmt_corpus_tests` only walks `examples/`. A typo in a section header or a
/// malformed JSON body would ship unnoticed.
///
/// Deliberately syntax-only. Many blocks are fragments illustrating one
/// section (`--- ENDPOINT ---` alone, an attribute before a `REQUEST`), so
/// full `check` validation would reject them for missing a RESPONSE — correctly,
/// but that is not what these snippets claim to be.
#[test]
fn every_docs_gctf_block_parses() {
    let blocks = support::markdown_gctf_blocks();
    assert!(!blocks.is_empty(), "expected ```gctf blocks in the docs");

    let mut failures = Vec::new();
    for (path, line, body) in &blocks {
        let label = format!("{}:{line}", path.display());
        if let Err(e) = grpctestify::parser::parse_gctf_from_str(body, &label) {
            failures.push(format!("{label}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} docs snippets do not parse:\n{}",
        failures.len(),
        blocks.len(),
        failures.join("\n")
    );
}

#[test]
fn cli_reference_lists_every_command() {
    let output = support::cli_command()
        .arg("--help")
        .output()
        .expect("failed to run --help");
    let help = String::from_utf8_lossy(&output.stdout);

    let commands: Vec<String> = help
        .split("Commands:")
        .nth(1)
        .unwrap_or_default()
        .split("Options:")
        .next()
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let t = l.strip_prefix("  ")?;
            let name = t.split_whitespace().next()?;
            // `help` is clap's own, not a documented feature.
            (name != "help" && name.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
                .then(|| name.to_string())
        })
        .collect();
    assert!(
        commands.len() > 10,
        "failed to parse the command list from --help: {commands:?}"
    );

    let reference = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/guides/reference/api/command-line.md"),
    )
    .expect("read CLI reference");
    let listed = reference
        .split("## Commands")
        .nth(1)
        .and_then(|s| s.split("## Global options").next())
        .expect("Commands section");

    let missing: Vec<&String> = commands
        .iter()
        .filter(|c| !listed.contains(&format!("`{c}")))
        .collect();
    assert!(
        missing.is_empty(),
        "commands missing from docs/guides/reference/api/command-line.md: {missing:?}"
    );
}

/// Docs must not teach the deprecated plugin spellings. `@uuid`, `@email`,
/// `@ip`, `@url`, `@timestamp` and `@empty` still work, but `check` reports
/// `SEM_D001` for them and `fmt --write` rewrites them to the `is_*` names —
/// so a snippet using them hands the reader code the tool immediately edits.
/// The API reference taught all five, and `basic-concepts.md` — the
/// introduction — used `@uuid` too.
///
/// Prose *about* the deprecation is fine; this only inspects ```gctf blocks.
#[test]
fn docs_snippets_use_canonical_plugin_names() {
    let deprecated = ["uuid", "email", "ip", "url", "timestamp", "empty"];
    let mut offenders = Vec::new();
    for (path, line, body) in support::markdown_gctf_blocks() {
        for name in deprecated {
            if body.contains(&format!("@{name}(")) {
                offenders.push(format!("{}:{line}: @{name}(", path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "docs snippets use deprecated plugin names (use the `is_*` form):\n{}",
        offenders.join("\n")
    );
}
