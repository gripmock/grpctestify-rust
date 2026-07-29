#![allow(clippy::unwrap_used, clippy::expect_used)] // test/bench code
#![cfg(not(miri))]

//! Plugin discovery via the convention directories — `~/.grpctestify/plugins`
//! (user-global) and `./.grpctestify/plugins` (project-local, the same
//! `.grpctestify/` `play --init` creates). There's no `--plugin-dir` flag —
//! every test here runs with an isolated `$HOME`/cwd (a fresh tempdir) so
//! tests can't see each other's plugins or the real host's `$HOME`.

#[path = "support/mod.rs"]
mod support;
use support::cli_command;

/// Spawn a real `grpc.health.v1.Health` server (plus reflection) on an
/// ephemeral port — same pattern as `tests/health_tests.rs`.
async fn spawn_health_server() -> String {
    let (reporter, health_service) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("build reflection service");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(reflection_service)
            .serve_with_incoming(incoming)
            .await
            .expect("health server run");
    });

    addr.to_string()
}

/// Write `is_present.rhai` into `<dir>/.grpctestify/plugins/` — the
/// project-local convention directory.
fn write_project_plugin(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        dir.join(".grpctestify/plugins/is_present.rhai"),
        "fn is_present(value) { value != () }",
    )
    .unwrap();
}

fn write_test_file(dir: &std::path::Path, address: &str) -> std::path::PathBuf {
    let file = dir.join("uses_plugin.gctf");
    std::fs::write(
        &file,
        format!(
            r#"--- ADDRESS ---
{address}

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{{}}

--- ASSERTS ---
@is_present(.status)
"#
        ),
    )
    .expect("failed to write test file");
    file
}

/// Run the CLI with `dir` as both the working directory and `$HOME` —
/// isolates the project-local and user-global convention dirs to `dir`.
fn run_isolated(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    cli_command()
        .current_dir(dir)
        .env("HOME", dir)
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(args)
        .output()
        .expect("failed to run CLI")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_finds_plugin_via_project_local_convention_dir() {
    let address = spawn_health_server().await;
    let test_dir = tempfile::tempdir().unwrap();
    write_project_plugin(test_dir.path());
    let file = write_test_file(test_dir.path(), &address);

    let output = run_isolated(test_dir.path(), &["run", &file.to_string_lossy()]);

    assert!(
        output.status.success(),
        "run must find the plugin under ./.grpctestify/plugins\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_without_any_plugin_source_reports_unknown_plugin() {
    let address = spawn_health_server().await;
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), &address);

    let output = run_isolated(test_dir.path(), &["run", &file.to_string_lossy()]);

    assert!(
        !output.status.success(),
        "an unregistered @is_present plugin must fail the run, not silently pass"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Unknown plugin") || combined.contains("is_present"),
        "failure must mention the unknown plugin: {combined}"
    );
}

#[test]
fn check_finds_plugin_via_project_local_convention_dir() {
    let test_dir = tempfile::tempdir().unwrap();
    write_project_plugin(test_dir.path());
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(
        test_dir.path(),
        &["check", &file.to_string_lossy(), "--format", "json"],
    );
    let json = support::parse_json_stdout_any_status(&output);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        !diagnostics
            .iter()
            .any(|d| d["message"].as_str().unwrap_or("").contains("is_present")),
        "known .rhai plugin must not be flagged unknown: {diagnostics:#?}"
    );
}

#[test]
fn check_without_any_plugin_source_flags_the_call_as_unknown() {
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(
        test_dir.path(),
        &["check", &file.to_string_lossy(), "--format", "json"],
    );
    let json = support::parse_json_stdout_any_status(&output);
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d["message"].as_str().unwrap_or("").contains("is_present")),
        "without a configured plugin source the same call must still be flagged unknown: {diagnostics:#?}"
    );
}

#[test]
fn fmt_finds_plugin_via_project_local_convention_dir() {
    let test_dir = tempfile::tempdir().unwrap();
    write_project_plugin(test_dir.path());
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["fmt", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "fmt must not fail on a known .rhai plugin call\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fmt_without_any_plugin_source_fails_on_the_unknown_plugin_call() {
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["fmt", &file.to_string_lossy()]);
    assert!(
        !output.status.success(),
        "without a configured plugin source, fmt must still fail on an unknown plugin call — \
         this pins down the regression where fmt hard-failed on every valid \
         .rhai-backed assertion instead of just check's diagnostics"
    );
}

#[test]
fn explain_finds_plugin_via_project_local_convention_dir() {
    let test_dir = tempfile::tempdir().unwrap();
    write_project_plugin(test_dir.path());
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["explain", &file.to_string_lossy()]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("Unknown assertion plugin"),
        "known .rhai plugin must not be flagged unknown by explain: {combined}"
    );
}

#[test]
fn explain_without_any_plugin_source_flags_the_call_as_unknown() {
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["explain", &file.to_string_lossy()]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Unknown assertion plugin") || combined.contains("is_present"),
        "without a configured plugin source explain must still flag the call as unknown: {combined}"
    );
}

#[test]
fn inspect_finds_plugin_via_project_local_convention_dir() {
    let test_dir = tempfile::tempdir().unwrap();
    write_project_plugin(test_dir.path());
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["inspect", &file.to_string_lossy()]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("Unknown assertion plugin"),
        "known .rhai plugin must not be flagged unknown by inspect: {combined}"
    );
}

#[test]
fn inspect_without_any_plugin_source_flags_the_call_as_unknown() {
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), "localhost:1");

    let output = run_isolated(test_dir.path(), &["inspect", &file.to_string_lossy()]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Unknown assertion plugin") || combined.contains("is_present"),
        "without a configured plugin source inspect must still flag the call as unknown: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_loads_plugins_from_the_user_global_convention_dir() {
    let address = spawn_health_server().await;
    let fake_home = tempfile::tempdir().unwrap();
    write_project_plugin(fake_home.path()); // writes under fake_home/.grpctestify/plugins

    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), &address);

    let output = cli_command()
        .current_dir(test_dir.path())
        .env("HOME", fake_home.path())
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");

    assert!(
        output.status.success(),
        "run must find the plugin under $HOME/.grpctestify/plugins even though it isn't the cwd\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_local_convention_dir_wins_over_the_user_global_one() {
    let address = spawn_health_server().await;
    let test_dir = tempfile::tempdir().unwrap();
    // User-global tier (== $HOME here): `is_present` always fails.
    std::fs::create_dir_all(test_dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        test_dir.path().join(".grpctestify/plugins/is_present.rhai"),
        "fn is_present(value) { false }",
    )
    .unwrap();
    // Project-local tier, in a *different* cwd: `is_present` always passes — must win.
    let project_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project_dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        project_dir
            .path()
            .join(".grpctestify/plugins/is_present.rhai"),
        "fn is_present(value) { true }",
    )
    .unwrap();
    let file = write_test_file(project_dir.path(), &address);

    let output = cli_command()
        .current_dir(project_dir.path())
        .env("HOME", test_dir.path())
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");

    assert!(
        output.status.success(),
        "project-local convention dir must take precedence over the user-global one\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn convention_dirs_are_never_created() {
    let address = spawn_health_server().await;
    let fake_home = tempfile::tempdir().unwrap();
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), &address);

    // No convention dirs exist anywhere — the run fails (unknown plugin),
    // but must not have created either directory as a side effect.
    let _ = cli_command()
        .current_dir(test_dir.path())
        .env("HOME", fake_home.path())
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");

    assert!(
        !fake_home.path().join(".grpctestify").exists(),
        "must not create $HOME/.grpctestify when it doesn't already exist"
    );
    assert!(
        !test_dir.path().join(".grpctestify").exists(),
        "must not create ./.grpctestify when it doesn't already exist"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grpctestify_home_env_var_overrides_the_global_dir() {
    let address = spawn_health_server().await;
    // The real $HOME (isolated to an empty tempdir) must NOT be consulted
    // once GRPCTESTIFY_HOME is set.
    let real_home = tempfile::tempdir().unwrap();
    let override_home = tempfile::tempdir().unwrap();
    write_project_plugin(override_home.path()); // writes under override_home/.grpctestify/plugins
    let test_dir = tempfile::tempdir().unwrap();
    let file = write_test_file(test_dir.path(), &address);

    let output = cli_command()
        .current_dir(test_dir.path())
        .env("HOME", real_home.path())
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .env("GRPCTESTIFY_HOME", override_home.path())
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");

    assert!(
        output.status.success(),
        "GRPCTESTIFY_HOME must redirect the global plugin dir\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grpctestify_project_dir_env_var_overrides_the_project_local_dir() {
    let address = spawn_health_server().await;
    let fake_home = tempfile::tempdir().unwrap();
    let override_project = tempfile::tempdir().unwrap();
    write_project_plugin(override_project.path()); // writes under override_project/.grpctestify/plugins
    // cwd is a *different* directory with no .grpctestify of its own.
    let cwd = tempfile::tempdir().unwrap();
    let file = write_test_file(cwd.path(), &address);

    let output = cli_command()
        .current_dir(cwd.path())
        .env("HOME", fake_home.path())
        .env("GRPCTESTIFY_TRUST_PLUGINS", "1")
        .env("GRPCTESTIFY_PROJECT_DIR", override_project.path())
        .args(["run", &file.to_string_lossy()])
        .output()
        .expect("failed to run CLI");

    assert!(
        output.status.success(),
        "GRPCTESTIFY_PROJECT_DIR must redirect the project-local plugin dir\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A `@pure`-tagged, `@returns bool` custom plugin must be eligible for the
/// same `== true` boolean rewrite (`fmt -O 2`) as a built-in bool plugin —
/// closes the gap where `apif-optimizer::BOOLEAN_PLUGINS` only ever knew
/// about built-ins, leaving custom plugins permanently ineligible regardless
/// of their doc tags.
#[test]
fn fmt_rewrites_a_pure_custom_plugin_the_same_as_a_builtin_bool_plugin() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        dir.path().join(".grpctestify/plugins/is_even.rhai"),
        "/// @param value: number\n/// @returns bool\n/// @pure\nfn is_even(value) { value % 2 == 0 }",
    )
    .unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\nexample.v1.Greeter/SayHello\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n@is_even(.count) == true\n",
    )
    .unwrap();

    let output = run_isolated(
        dir.path(),
        &["fmt", "-w", "-O", "2", &file.to_string_lossy()],
    );
    assert!(
        output.status.success(),
        "fmt -w -O 2 failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let updated = std::fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("@is_even(.count)") && !updated.contains("== true"),
        "a @pure/@returns bool custom plugin must have `== true` rewritten away: {updated}"
    );
}

/// An untagged custom plugin (no `@pure`) must stay conservative — never
/// used as the basis for the boolean rewrite, matching every built-in
/// plugin without `safe_for_rewrite`.
#[test]
fn fmt_does_not_rewrite_an_untagged_custom_plugin() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".grpctestify/plugins")).unwrap();
    std::fs::write(
        dir.path().join(".grpctestify/plugins/is_even.rhai"),
        "fn is_even(value) { value % 2 == 0 }",
    )
    .unwrap();
    let file = dir.path().join("t.gctf");
    std::fs::write(
        &file,
        "--- ENDPOINT ---\nexample.v1.Greeter/SayHello\n\n--- REQUEST ---\n{}\n\n--- ASSERTS ---\n@is_even(.count) == true\n",
    )
    .unwrap();

    let output = run_isolated(
        dir.path(),
        &["fmt", "-w", "-O", "2", &file.to_string_lossy()],
    );
    assert!(
        output.status.success(),
        "fmt -w -O 2 failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let updated = std::fs::read_to_string(&file).unwrap();
    assert!(
        updated.contains("@is_even(.count) == true"),
        "an untagged custom plugin must NOT get the boolean rewrite: {updated}"
    );
}

/// `plugins install`'s storage layout (`installed/<host>/<owner>/<repo>/`)
/// without going over the network — same layout `install_source` writes to,
/// exercised here by placing the files directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_finds_a_plugin_under_the_installed_subdirectory() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(
        dir.path()
            .join(".grpctestify/plugins/installed/github.com/owner/repo"),
    )
    .unwrap();
    std::fs::write(
        dir.path()
            .join(".grpctestify/plugins/installed/github.com/owner/repo/is_present.rhai"),
        "fn is_present(value) { value != () }",
    )
    .unwrap();
    let file = write_test_file(dir.path(), &address);

    let output = run_isolated(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "an installed plugin must be usable the same as a hand-authored one\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A hand-authored script directly under `plugins/` must win over an
/// installed one of the same name in the same tier.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hand_authored_plugin_wins_over_an_installed_plugin_of_the_same_name() {
    let address = spawn_health_server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(
        dir.path()
            .join(".grpctestify/plugins/installed/github.com/owner/repo"),
    )
    .unwrap();
    // The installed copy always fails; the hand-authored one always passes —
    // whichever wins is unambiguous from the run's exit code alone.
    std::fs::write(
        dir.path()
            .join(".grpctestify/plugins/installed/github.com/owner/repo/is_present.rhai"),
        "fn is_present(_value) { false }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".grpctestify/plugins/is_present.rhai"),
        "fn is_present(value) { value != () }",
    )
    .unwrap();
    let file = write_test_file(dir.path(), &address);

    let output = run_isolated(dir.path(), &["run", &file.to_string_lossy()]);
    assert!(
        output.status.success(),
        "the hand-authored plugin must win over the installed one of the same name\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
