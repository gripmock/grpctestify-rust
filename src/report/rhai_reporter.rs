//! `Reporter` implementations backed by user-authored `.rhai` scripts, loaded
//! from the same convention directories as assertion plugins (see
//! `apif_plugins::rhai_plugin::{user_plugin_dir, project_plugin_dir}`).
//!
//! A script opts into being a reporter simply by defining one or more of
//! `on_test_start(name)`, `on_test_end(name, result)`, `on_suite_end(results)`
//! — no separate flag or directory needed; a script can be an assertion
//! plugin (`fn check(...)`), a reporter, or (unusually) both, purely by
//! which functions it declares. `result`/`results` are the same
//! `TestResult`/`TestResults` shape every other reporter sees, serialized to
//! a rhai value. Output happens through rhai's own `print`/`debug` (wired to
//! stdout/stderr by `Engine::new()` by default) — no file/network access is
//! granted, matching the assertion-plugin side's pure-function-only v1 scope.

use anyhow::{Context, Result};
use apif_report::Reporter;
use apif_state::{TestResult, TestResults};
use rhai::{AST, Engine};
use std::path::Path;

pub struct RhaiReporter {
    name: String,
    engine: Engine,
    ast: AST,
    has_on_test_start: bool,
    has_on_test_end: bool,
    has_on_suite_end: bool,
}

impl RhaiReporter {
    /// `None` when the script defines none of the three Reporter hooks —
    /// not every `.rhai` file under `--plugin-dir` is a reporter (most are
    /// assertion plugins), so this isn't an error, just "not applicable".
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .with_context(|| format!("plugin script has no file name: {}", path.display()))?;

        let engine = apif_plugins::rhai_stdlib::build_engine();
        let ast = engine
            .compile_file(path.to_path_buf())
            .with_context(|| format!("failed to compile plugin script: {}", path.display()))?;

        let has_on_test_start = ast.iter_functions().any(|f| f.name == "on_test_start");
        let has_on_test_end = ast.iter_functions().any(|f| f.name == "on_test_end");
        let has_on_suite_end = ast.iter_functions().any(|f| f.name == "on_suite_end");

        if !has_on_test_start && !has_on_test_end && !has_on_suite_end {
            return Ok(None);
        }

        Ok(Some(Self {
            name,
            engine,
            ast,
            has_on_test_start,
            has_on_test_end,
            has_on_suite_end,
        }))
    }
}

impl RhaiReporter {
    /// Real logic behind `Reporter::on_test_start` — split out so tests can
    /// observe the `Result` directly (the trait method returns `()`, so a
    /// wrong-data bug inside the script would otherwise only ever show up as
    /// a `tracing::error!` log, invisible to a unit test).
    fn call_on_test_start(&self, test_name: &str) -> Result<()> {
        if !self.has_on_test_start {
            return Ok(());
        }
        self.engine
            .call_fn::<()>(
                &mut rhai::Scope::new(),
                &self.ast,
                "on_test_start",
                (test_name.to_string(),),
            )
            .map_err(|e| anyhow::anyhow!("reporter '{}' on_test_start error: {e}", self.name))
    }

    fn call_on_test_end(&self, test_name: &str, result: &TestResult) -> Result<()> {
        if !self.has_on_test_end {
            return Ok(());
        }
        let value = serde_json::to_value(result).context("failed to serialize test result")?;
        let dynamic = rhai::serde::to_dynamic(&value).context("failed to convert test result")?;
        self.engine
            .call_fn::<()>(
                &mut rhai::Scope::new(),
                &self.ast,
                "on_test_end",
                (test_name.to_string(), dynamic),
            )
            .map_err(|e| anyhow::anyhow!("reporter '{}' on_test_end error: {e}", self.name))
    }

    fn call_on_suite_end(&self, results: &TestResults) -> Result<()> {
        if !self.has_on_suite_end {
            return Ok(());
        }
        let value = serde_json::to_value(results).context("failed to serialize suite results")?;
        let dynamic = rhai::serde::to_dynamic(&value).context("failed to convert suite results")?;
        self.engine
            .call_fn::<()>(
                &mut rhai::Scope::new(),
                &self.ast,
                "on_suite_end",
                (dynamic,),
            )
            .map_err(|e| anyhow::anyhow!("reporter '{}' on_suite_end error: {e}", self.name))
    }
}

impl Reporter for RhaiReporter {
    fn on_test_start(&self, test_name: &str) {
        if let Err(e) = self.call_on_test_start(test_name) {
            tracing::error!("{e}");
        }
    }

    fn on_test_end(&self, test_name: &str, result: &TestResult) {
        if let Err(e) = self.call_on_test_end(test_name, result) {
            tracing::error!("{e}");
        }
    }

    fn on_suite_end(&self, results: &TestResults) -> Result<()> {
        self.call_on_suite_end(results)
    }
}

/// Load every `.rhai` file directly under `dir` that declares at least one
/// Reporter hook. A file with no hooks (an assertion plugin, or a script
/// that fails to compile) is silently skipped here — `rhai_plugin` covers
/// the assertion-plugin side of the same directory.
pub fn load_rhai_reporters(dir: &Path) -> Vec<Box<dyn Reporter>> {
    let mut reporters: Vec<Box<dyn Reporter>> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("failed to read plugin directory {}: {e}", dir.display());
            return reporters;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rhai") {
            continue;
        }
        match RhaiReporter::load(&path) {
            Ok(Some(reporter)) => reporters.push(Box::new(reporter)),
            Ok(None) => {}
            Err(e) => tracing::error!("skipping reporter script {}: {e:#}", path.display()),
        }
    }
    reporters
}

/// Load reporter scripts from every configured convention directory — the
/// user-global and project-local plugin dirs
/// (`apif_plugins::rhai_plugin::user_plugin_dir`/`project_plugin_dir`).
/// Unlike assertion plugins, reporters aren't name-keyed — a script
/// defining the same hooks in both tiers just runs as two active
/// reporters, no precedence/override needed.
pub fn load_all_configured_reporters() -> Vec<Box<dyn Reporter>> {
    let mut reporters = Vec::new();
    let project_dir = apif_plugins::rhai_plugin::project_plugin_dir();
    if project_dir.is_dir() {
        reporters.extend(load_rhai_reporters(&project_dir));
    }
    if let Some(dir) = apif_plugins::rhai_plugin::user_plugin_dir()
        && dir.is_dir()
    {
        reporters.extend(load_rhai_reporters(&dir));
    }
    reporters
}

#[cfg(test)]
mod tests {
    use super::*;
    use apif_state::TestMeta;

    #[cfg(not(miri))]
    fn write_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    #[cfg(not(miri))]
    fn assertion_only_script_is_not_a_reporter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "assert_only.rhai", "fn check() { true }");
        assert!(RhaiReporter::load(&path).unwrap().is_none());
    }

    #[test]
    #[cfg(not(miri))]
    fn script_with_any_hook_is_detected_as_a_reporter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "reporter.rhai",
            "fn on_suite_end(results) { () }",
        );
        assert!(RhaiReporter::load(&path).unwrap().is_some());
    }

    #[test]
    #[cfg(not(miri))]
    fn on_test_end_receives_the_real_name_and_status() {
        let dir = tempfile::tempdir().unwrap();
        // Throws (call_fn errors) if the received data doesn't match —
        // asserted directly via `call_on_test_end`'s `Result`, not just
        // "didn't panic" (the trait method itself returns `()`).
        let path = write_script(
            dir.path(),
            "check_data.rhai",
            r#"fn on_test_end(name, result) {
                if name != "svc.Thing/Do" { throw "wrong name: " + name; }
                if result.status != "Fail" { throw "wrong status: " + result.status; }
                if result.duration_ms != 42 { throw "wrong duration: " + result.duration_ms; }
            }"#,
        );
        let reporter = RhaiReporter::load(&path).unwrap().unwrap();
        let result = TestResult::fail("svc.Thing/Do", "boom".into(), 42, None);
        reporter.call_on_test_end("svc.Thing/Do", &result).unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn on_test_end_actually_fails_when_data_is_wrong() {
        // Proves the throw mechanism the test above relies on really does
        // propagate as an `Err` — without this, a bug that fed the script
        // the wrong data would go unnoticed by every "throws if wrong" test.
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "always_wrong.rhai",
            r#"fn on_test_end(name, result) { throw "deliberately wrong"; }"#,
        );
        let reporter = RhaiReporter::load(&path).unwrap().unwrap();
        let result = TestResult::pass("a.gctf", 5, None);
        let err = reporter.call_on_test_end("a.gctf", &result).unwrap_err();
        assert!(err.to_string().contains("deliberately wrong"), "{err}");
    }

    #[test]
    #[cfg(not(miri))]
    fn on_suite_end_receives_aggregate_counts_and_full_results() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "suite.rhai",
            r#"fn on_suite_end(results) {
                if results.total != 2 { throw "wrong total: " + results.total; }
                if results.passed != 1 { throw "wrong passed: " + results.passed; }
                if results.failed != 1 { throw "wrong failed: " + results.failed; }
                if results.results.len() != 2 { throw "wrong results length"; }
            }"#,
        );
        let reporter = RhaiReporter::load(&path).unwrap().unwrap();

        let mut results = TestResults::new();
        results.add(TestResult::pass("a.gctf", 5, None));
        results.add(TestResult::fail("b.gctf", "boom".into(), 5, None));

        reporter.on_suite_end(&results).unwrap();
    }

    #[test]
    #[cfg(not(miri))]
    fn a_hook_that_throws_surfaces_as_an_error_from_on_suite_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "throws.rhai",
            "fn on_suite_end(r) { throw \"boom\"; }",
        );
        let reporter = RhaiReporter::load(&path).unwrap().unwrap();
        let err = reporter.on_suite_end(&TestResults::new()).unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[test]
    #[cfg(not(miri))]
    fn meta_and_optional_fields_serialize_without_error() {
        // Regression: TestResult carries several Option/skip_serializing_if
        // fields (meta, exchange, retried, ...) — confirm a populated one
        // still converts cleanly through serde_json -> rhai::serde.
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "meta.rhai",
            r#"fn on_test_end(name, result) {
                if result.meta.owner != "team-x" { throw "missing meta: " + result.meta.owner; }
            }"#,
        );
        let reporter = RhaiReporter::load(&path).unwrap().unwrap();
        let mut result = TestResult::pass("a.gctf", 5, None);
        result.meta = TestMeta {
            owner: Some("team-x".to_string()),
            ..Default::default()
        };
        reporter.call_on_test_end("a.gctf", &result).unwrap();
    }
}
