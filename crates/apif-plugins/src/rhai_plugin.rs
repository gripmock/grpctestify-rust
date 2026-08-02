//! Assertion plugins backed by user-authored `.rhai` scripts, loaded from
//! the convention directories — [`user_plugin_dir`] (`~/.grpctestify/plugins`)
//! and [`project_plugin_dir`] (`./.grpctestify/plugins`); see
//! [`load_all_configured_plugins`].
//!
//! v1 contract: every public top-level function in a script becomes its own
//! plugin, named directly after itself — `fn is_even(x) {...}` anywhere
//! under one of those directories becomes `@is_even(...)`. One file can
//! define many (no single magic function name like `check`); the file it
//! lives in is just where the definition happens to be, not part of the
//! plugin's name.
//! `private fn` (Rhai's own visibility keyword) stays internal — a helper
//! used by other functions in the file, never itself exposed as a plugin.
//! `on_test_start`/`on_test_end`/`on_suite_end` are reserved for the
//! separate Reporter contract ([`crate::rhai_stdlib`]'s sibling in
//! `rhai_reporter.rs`) and never become assertion plugins even if public.
//! Two public functions (in the same file or different files) sharing a
//! name is a load-time conflict — see [`load_rhai_plugins`].
//!
//! Each function takes one named parameter per assertion argument — `fn
//! check(value)`, `fn check(value, min, max)`, whatever arity the call site
//! uses — and returns either a `bool` (pass/fail) or any other value
//! (composed further, e.g. inside a jq expression). Dispatch is by arity,
//! same as Rhai's own function overloading: a script can define more than
//! one overload of the same function name to serve more than one call
//! shape. A `///` doc comment immediately above a function becomes that
//! plugin's `description()`.
//!
//! No access to the response/headers/trailers — pure functions only, so a
//! script-backed plugin behaves identically whether called directly or from
//! inside a jq subexpression.
//!
//! # Type tags (optional, trust-based)
//!
//! A doc comment MAY also carry `@param <name>: <type>`, `@returns <type>`,
//! and `@pure` tags (`<type>` is one of `bool`/`uint`/`number`/`string`/
//! `time`/`json`/`yaml`, case-insensitive):
//!
//! ```rhai
//! /// Checks a value is within [min, max].
//! /// @param value: number
//! /// @param min: number
//! /// @param max: number
//! /// @returns bool
//! /// @pure
//! fn in_range(value, min, max) { value >= min && value <= max }
//! ```
//!
//! These aren't verified against the script body — a script author can
//! claim `@pure`/a wrong return type and nothing here catches it (the same
//! trust boundary `unsafe` is in Rust: a promise, not a proof). Without
//! `@pure`, a plugin's purity/determinism stays exactly as conservative as
//! before (never used as the basis for an optimizer rewrite). Untagged
//! parameters/return default to `Any`, same as today.
//!
//! A bare `@inline_option` tag makes the plugin's own name a valid inline
//! option key on a section header (`--- RESPONSE my_check=5 ---`) instead of
//! the parser rejecting it as unknown — see [`load_all_inline_option_keys`].

use anyhow::Result;
use apif_assert::engine::AssertionResult;
use rhai::{AST, Dynamic, Engine, FnAccess};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use crate::{
    ArgTypeInfo, Plugin, PluginContext, PluginPurity, PluginResult, PluginSignature, TypeInfo,
};

/// Reporter hooks — never exposed as assertion plugins even though they're
/// public functions, so a reporter script doesn't also register itself
/// under `@on_test_end` etc.
const RESERVED_FN_NAMES: &[&str] = &["on_test_start", "on_test_end", "on_suite_end"];

const DEFAULT_DESCRIPTION: &str = "User-defined assertion plugin (.rhai script)";

pub struct RhaiPlugin {
    /// Also the Rhai function this plugin calls — the plugin's name IS the
    /// function's own name, one to one.
    name: String,
    description: String,
    /// Shared across every plugin loaded from the same file — one compile,
    /// many plugin views into it.
    engine: Arc<Engine>,
    ast: Arc<AST>,
    arg_names: &'static [&'static str],
    arg_types: &'static [ArgTypeInfo],
    /// From an `@returns <type>` doc tag — `Any` if untagged.
    return_type: TypeInfo,
    /// From a `@pure` doc tag. Drives `purity`/`deterministic`/`idempotent`/
    /// `safe_for_rewrite` — see [`Plugin::signature`]. A script's behavior
    /// can't actually be verified, so this is a trust boundary: `@pure` is
    /// a claim, not a proof.
    pure: bool,
    /// From an `@inline_option` doc tag — this plugin's own name is also
    /// accepted as an inline-option key on a section header (e.g.
    /// `--- RESPONSE my_check=5 ---`). See [`load_all_inline_option_keys`].
    inline_option: bool,
    /// Resolved on first execution, not at load, so `check` never asks.
    path: std::path::PathBuf,
    digest: Arc<str>,
    trusted: Arc<std::sync::OnceLock<bool>>,
}

impl RhaiPlugin {
    fn is_trusted(&self) -> bool {
        *self
            .trusted
            .get_or_init(|| crate::trust::is_trusted(&self.path, &self.digest))
    }

    /// Compile a `.rhai` file and return one [`RhaiPlugin`] per public
    /// top-level function (excluding reporter hooks). Empty (not an error)
    /// when the file defines no public functions — it might be a
    /// reporter-only script. Fails only if the file doesn't parse.
    pub fn load_all(path: &Path) -> Result<Vec<Self>> {
        let engine = crate::rhai_stdlib::build_engine();
        let (ast, digest) = crate::rhai_stdlib::compile_with_digest(&engine, path)?;

        let engine = Arc::new(engine);
        let ast = Arc::new(ast);

        let mut seen = std::collections::HashSet::new();
        let mut plugins = Vec::new();
        let trusted: Arc<std::sync::OnceLock<bool>> = Arc::new(std::sync::OnceLock::new());
        let digest: Arc<str> = Arc::from(digest.as_str());

        for f in ast.iter_functions() {
            if f.access != FnAccess::Public || RESERVED_FN_NAMES.contains(&f.name) {
                continue;
            }
            // Multiple arity overloads share one name — only the first
            // occurrence (source order) becomes the plugin's arg_names/doc.
            if !seen.insert(f.name.to_string()) {
                continue;
            }

            let arg_names = leak_arg_names(&f.params);
            let doc = parse_doc_comment(&f.comments, &f.params);
            let arg_types = leak_arg_types(arg_names.len(), &doc.param_types);

            plugins.push(Self {
                name: f.name.to_string(),
                description: doc
                    .description
                    .unwrap_or_else(|| DEFAULT_DESCRIPTION.to_string()),
                engine: engine.clone(),
                ast: ast.clone(),
                arg_names,
                arg_types,
                return_type: doc.return_type,
                pure: doc.pure,
                inline_option: doc.inline_option,
                path: path.to_path_buf(),
                digest: digest.clone(),
                trusted: trusted.clone(),
            });
        }

        Ok(plugins)
    }
}

/// Parsed from a function's `///` doc comment: the human-readable
/// description (every non-tag line, joined) plus whichever `@param`/
/// `@returns`/`@pure` tags were present — see the module-level "Type tags"
/// section for the exact syntax.
struct ParsedDoc {
    description: Option<String>,
    /// Parallel to `params` — `None` where a parameter had no `@param` tag.
    param_types: Vec<Option<TypeInfo>>,
    return_type: TypeInfo,
    pure: bool,
    inline_option: bool,
}

fn parse_type_tag(token: &str) -> TypeInfo {
    match token.trim().to_ascii_lowercase().as_str() {
        "bool" | "boolean" => TypeInfo::Bool,
        "uint" => TypeInfo::UInt,
        "number" | "num" | "int" | "float" => TypeInfo::Number,
        "string" | "str" => TypeInfo::String,
        "time" | "timestamp" | "duration" => TypeInfo::Time,
        "json" => TypeInfo::Json,
        "yaml" => TypeInfo::Yaml,
        _ => TypeInfo::Any,
    }
}

/// Strip Rhai's `///`/`/**`-block comment markers, then split each line
/// into a doc-tag (`@param`/`@returns`/`@pure`) or plain description text.
/// Unrecognized types/tags are ignored, not errors — this is best-effort,
/// trust-based metadata, not a strict schema.
fn parse_doc_comment(comments: &[&str], params: &[&str]) -> ParsedDoc {
    let mut description_lines = Vec::new();
    let mut param_types: Vec<Option<TypeInfo>> = vec![None; params.len()];
    let mut return_type = TypeInfo::Any;
    let mut pure = false;
    let mut inline_option = false;

    // Rhai merges a contiguous `///` block into one entry per `comments`
    // slot, with the original line breaks kept as embedded `\n` — split
    // back into individual lines before tag-matching each one.
    for raw in comments.iter().flat_map(|c| c.split('\n')) {
        let line = raw
            .trim()
            .trim_start_matches("///")
            .trim_start_matches("/**")
            .trim_end_matches("*/")
            .trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("@param ") {
            if let Some((name, ty)) = rest.split_once(':') {
                let name = name.trim();
                if let Some(idx) = params.iter().position(|p| *p == name) {
                    param_types[idx] = Some(parse_type_tag(ty));
                }
            }
        } else if let Some(rest) = line.strip_prefix("@returns ") {
            return_type = parse_type_tag(rest);
        } else if line == "@pure" {
            pure = true;
        } else if line == "@inline_option" {
            inline_option = true;
        } else {
            description_lines.push(line.to_string());
        }
    }

    ParsedDoc {
        description: (!description_lines.is_empty()).then(|| description_lines.join(" ")),
        param_types,
        return_type,
        pure,
        inline_option,
    }
}

/// Copy Rhai's borrowed parameter names out to `'static` — leaked once per
/// plugin at load time (a bounded, one-time cost for the process lifetime,
/// not a per-call or per-reload allocation).
fn leak_arg_names(params: &[&str]) -> &'static [&'static str] {
    let owned: Vec<&'static str> = params
        .iter()
        .map(|p| -> &'static str { Box::leak(p.to_string().into_boxed_str()) })
        .collect();
    Box::leak(owned.into_boxed_slice())
}

/// `param_types[i]` (from an `@param` tag), or `Any` when the parameter
/// wasn't tagged.
fn leak_arg_types(count: usize, param_types: &[Option<TypeInfo>]) -> &'static [ArgTypeInfo] {
    let types: Vec<ArgTypeInfo> = (0..count)
        .map(|i| ArgTypeInfo {
            expected: param_types
                .get(i)
                .copied()
                .flatten()
                .unwrap_or(TypeInfo::Any),
            required: true,
            default: None,
        })
        .collect();
    Box::leak(types.into_boxed_slice())
}

impl Plugin for RhaiPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn signature(&self) -> PluginSignature {
        PluginSignature {
            return_type: self.return_type,
            arg_types: self.arg_types,
            // A script's behavior can't actually be verified — `@pure` is a
            // trust boundary (a claim, not a proof); untagged scripts keep
            // the fully conservative defaults so the optimizer never
            // rewrites around one.
            purity: if self.pure {
                PluginPurity::Pure
            } else {
                PluginPurity::Impure
            },
            deterministic: self.pure,
            idempotent: self.pure,
            safe_for_rewrite: self.pure,
            arg_names: self.arg_names,
            replacement: None,
        }
    }

    fn execute(&self, args: &[Value], _context: &PluginContext) -> Result<PluginResult> {
        if !self.is_trusted() {
            anyhow::bail!(
                "@{}: refusing to execute untrusted plugin script {}",
                self.name,
                self.path.display()
            );
        }
        let rhai_args: Vec<Dynamic> = args
            .iter()
            .map(rhai::serde::to_dynamic)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow::anyhow!("failed to convert arguments for @{}: {e}", self.name))?;

        let result: Dynamic = self
            .engine
            .call_fn(&mut rhai::Scope::new(), &self.ast, &self.name, rhai_args)
            .map_err(|e| {
                anyhow::anyhow!(
                    "@{} script error (called with {} argument(s)): {e}",
                    self.name,
                    args.len()
                )
            })?;

        if let Some(b) = result.clone().try_cast::<bool>() {
            return Ok(PluginResult::Assertion(if b {
                AssertionResult::Pass
            } else {
                AssertionResult::fail(format!("@{} returned false", self.name))
            }));
        }

        let json = rhai::serde::from_dynamic::<Value>(&result).map_err(|e| {
            anyhow::anyhow!(
                "@{} returned a value that can't convert to JSON: {e}",
                self.name
            )
        })?;
        Ok(PluginResult::Value(json))
    }
}

/// Load every `.rhai` file directly under `dir`, registering one plugin per
/// public function found (see [`RhaiPlugin::load_all`]). A file that fails
/// to compile is skipped with a logged error rather than aborting the whole
/// load — one broken script shouldn't take down every other plugin.
///
/// Two public functions sharing a name — same file or different files — is
/// a real conflict, not a "last one wins" situation: silently shadowing one
/// plugin with another means `@name(...)` calls whichever loaded last,
/// which depends on directory read order. Files are processed in sorted
/// order for determinism, and the *first* definition of a name wins; every
/// later conflicting one is skipped with a loud `tracing::error!` naming
/// both files, so the conflict is visible even though this loader can't
/// fail outright — it runs inside a `LazyLock` initializer
/// ([`crate::execution::plugin_dir`] in the main crate), which can't
/// propagate a `Result`.
pub fn load_rhai_plugins(dir: &Path) -> Vec<Arc<dyn Plugin>> {
    load_rhai_plugins_typed(dir)
        .into_iter()
        .map(|p| Arc::new(p) as Arc<dyn Plugin>)
        .collect()
}

/// Same as [`load_rhai_plugins`], but returns the concrete [`RhaiPlugin`]
/// (not yet erased to `Arc<dyn Plugin>`) — needed by callers that read a
/// `RhaiPlugin`-only field like `inline_option` (the shared `Plugin` trait
/// doesn't carry it).
fn load_rhai_plugins_typed(dir: &Path) -> Vec<RhaiPlugin> {
    let mut plugins: Vec<RhaiPlugin> = Vec::new();
    let mut owner: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("failed to read plugin directory {}: {e}", dir.display());
            return plugins;
        }
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rhai"))
        .collect();
    paths.sort();

    for path in paths {
        match RhaiPlugin::load_all(&path) {
            Ok(found) => {
                for plugin in found {
                    match owner.get(plugin.name()) {
                        Some(existing) => {
                            tracing::error!(
                                "skipping @{}: already defined in {} (also found in {})",
                                plugin.name(),
                                existing.display(),
                                path.display()
                            );
                        }
                        None => {
                            tracing::debug!(
                                "loaded plugin @{} from {}",
                                plugin.name(),
                                path.display()
                            );
                            owner.insert(plugin.name().to_string(), path.clone());
                            plugins.push(plugin);
                        }
                    }
                }
            }
            Err(e) => tracing::error!("skipping plugin script {}: {e:#}", path.display()),
        }
    }
    plugins
}

/// The user's home directory, resolved without a dependency: `$HOME` first
/// (set on every Unix-like OS, and often on Windows too under
/// Git-Bash/MSYS/WSL-adjacent shells), falling back to `%USERPROFILE%` on
/// Windows (native `cmd`/PowerShell don't set `$HOME`).
fn home_dir() -> Option<std::path::PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(std::path::PathBuf::from(home));
    }
    if cfg!(windows)
        && let Ok(profile) = std::env::var("USERPROFILE")
        && !profile.is_empty()
    {
        return Some(std::path::PathBuf::from(profile));
    }
    None
}

/// Non-empty string from `var`, if set — the shared helper behind both
/// directory env-var overrides below.
fn non_empty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

/// `<home>/.grpctestify/plugins` — the user-global settings directory
/// (`.grpctestify/` isn't plugin-specific; `plugins/` is the one subpath
/// this crate reads). Same directory on every OS (Windows tolerates a
/// dot-prefixed directory name exactly like `.cargo`/`.rustup`/`.ssh`
/// already do) — no XDG/AppData branching. `GRPCTESTIFY_HOME` stands in for
/// `<home>` (not for `.grpctestify` itself), same meaning as `$HOME`, just
/// overridable. Never created by this crate — only read if it already
/// exists.
pub fn user_plugin_dir() -> Option<std::path::PathBuf> {
    Some(user_state_dir()?.join("plugins"))
}

/// `<home>/.grpctestify`. Separate from [`user_plugin_dir`] so the trust store
/// still resolves on a machine where `plugins/` was never created.
pub fn user_state_dir() -> Option<std::path::PathBuf> {
    let home = non_empty_env("GRPCTESTIFY_HOME")
        .map(std::path::PathBuf::from)
        .or_else(home_dir)?;
    Some(home.join(".grpctestify"))
}

/// `<cwd>/.grpctestify/plugins` — the project-local settings directory, the
/// same `.grpctestify/` a project already gets from `play --init`.
/// `GRPCTESTIFY_PROJECT_DIR` stands in for `<cwd>` (e.g. to point at a repo
/// root from a monorepo subdirectory without `cd`-ing there). Never created
/// by this crate — only read if it already exists.
pub fn project_plugin_dir() -> std::path::PathBuf {
    let cwd = non_empty_env("GRPCTESTIFY_PROJECT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    cwd.join(".grpctestify").join("plugins")
}

/// Leaf directories under `<plugins_dir>/installed/<host>/<owner>/<repo>`,
/// as populated by `plugins install` — sorted for deterministic load order.
fn installed_leaf_dirs(plugins_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut leaves = Vec::new();
    for host in read_subdirs(&plugins_dir.join("installed")) {
        for owner in read_subdirs(&host) {
            leaves.extend(read_subdirs(&owner));
        }
    }
    leaves.sort();
    leaves
}

fn read_subdirs(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

/// Load every configured plugin source, lowest to highest precedence:
/// [`user_plugin_dir`] (global default) → [`project_plugin_dir`]
/// (project-local, same `.grpctestify/` `play --init` creates). A name
/// defined in both is not a load-time conflict the way two names colliding
/// *within* one directory are ([`load_rhai_plugins`]) — the more specific
/// (project-local) tier silently wins, the same local-overrides-global
/// precedence most tools give a project config over a user-wide default
/// (npm: project > user; git: local > global > system). Within one tier,
/// hand-authored scripts (directly under the tier's directory) outrank
/// `plugins install`ed ones (under `installed/<host>/<owner>/<repo>/`).
pub fn load_all_configured_plugins() -> Vec<Arc<dyn Plugin>> {
    load_all_configured_plugins_typed()
        .into_iter()
        .map(|p| Arc::new(p) as Arc<dyn Plugin>)
        .collect()
}

/// Names of `.rhai` plugins whose doc comment carries a bare `@inline_option`
/// tag — each becomes a valid inline-option key on a section header (e.g.
/// `--- RESPONSE my_check=5 ---`) that the parser would otherwise reject as
/// unknown. Same precedence/dedup rules as [`load_all_configured_plugins`].
pub fn load_all_inline_option_keys() -> std::collections::HashSet<String> {
    load_all_configured_plugins_typed()
        .into_iter()
        .filter(|p| p.inline_option)
        .map(|p| p.name)
        .collect()
}

fn load_all_configured_plugins_typed() -> Vec<RhaiPlugin> {
    let mut plugins: Vec<RhaiPlugin> = Vec::new();
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in [project_plugin_dir()]
        .into_iter()
        .chain(user_plugin_dir())
        .filter(|dir| dir.is_dir())
    {
        for plugin in load_rhai_plugins_typed(&dir) {
            if names.insert(plugin.name.clone()) {
                plugins.push(plugin);
            }
        }
        for leaf in installed_leaf_dirs(&dir) {
            for plugin in load_rhai_plugins_typed(&leaf) {
                if names.insert(plugin.name.clone()) {
                    plugins.push(plugin);
                }
            }
        }
    }

    plugins
}

#[cfg(test)]
mod tests {
    /// `load_all` + pre-approve, so a temp script doesn't hit `crate::trust`.
    fn load_trusted(path: &std::path::Path) -> anyhow::Result<Vec<super::RhaiPlugin>> {
        let plugins = super::RhaiPlugin::load_all(path)?;
        for p in &plugins {
            let _ = p.trusted.set(true);
        }
        Ok(plugins)
    }

    use super::*;

    #[cfg(not(miri))]
    fn write_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn ctx() -> PluginContext<'static> {
        PluginContext::new(&Value::Null)
    }

    fn find<'a>(plugins: &'a [RhaiPlugin], name: &str) -> &'a RhaiPlugin {
        plugins
            .iter()
            .find(|p| p.name() == name)
            .unwrap_or_else(|| {
                panic!(
                    "no plugin named {name}, found: {:?}",
                    plugins.iter().map(Plugin::name).collect::<Vec<_>>()
                )
            })
    }

    #[test]
    #[cfg(not(miri))]
    fn loads_and_evaluates_a_boolean_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "is_even.rhai", "fn is_even(x) { x % 2 == 0 }");
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];
        assert_eq!(plugin.name(), "is_even");

        let pass = plugin.execute(&[Value::Number(4.into())], &ctx()).unwrap();
        assert!(matches!(
            pass,
            PluginResult::Assertion(AssertionResult::Pass)
        ));

        let fail = plugin.execute(&[Value::Number(3.into())], &ctx()).unwrap();
        assert!(matches!(
            fail,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    #[cfg(not(miri))]
    fn non_boolean_return_becomes_a_value_result() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "double.rhai", "fn double(x) { x * 2 }");
        let plugins = load_trusted(&path).unwrap();
        let plugin = &plugins[0];

        let result = plugin.execute(&[Value::Number(21.into())], &ctx()).unwrap();
        match result {
            PluginResult::Value(v) => assert_eq!(v, Value::Number(42.into())),
            other => panic!("expected Value, got {other:?}"),
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn dispatches_by_arity_with_named_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "in_range.rhai",
            "fn in_range(value, min, max) { value >= min && value <= max }",
        );
        let plugins = load_trusted(&path).unwrap();
        let plugin = &plugins[0];
        assert_eq!(plugin.signature().arg_names, &["value", "min", "max"]);

        let pass = plugin
            .execute(
                &[
                    Value::Number(30.into()),
                    Value::Number(18.into()),
                    Value::Number(65.into()),
                ],
                &ctx(),
            )
            .unwrap();
        assert!(matches!(
            pass,
            PluginResult::Assertion(AssertionResult::Pass)
        ));
    }

    #[test]
    #[cfg(not(miri))]
    fn a_script_can_overload_check_across_arities() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "flexible.rhai",
            "fn flexible(x) { x > 0 }\nfn flexible(x, y) { x > y }",
        );
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(
            plugins.len(),
            1,
            "arity overloads share one plugin identity"
        );
        let plugin = &plugins[0];

        let one_arg = plugin.execute(&[Value::Number(1.into())], &ctx()).unwrap();
        assert!(matches!(
            one_arg,
            PluginResult::Assertion(AssertionResult::Pass)
        ));

        let two_args = plugin
            .execute(&[Value::Number(5.into()), Value::Number(10.into())], &ctx())
            .unwrap();
        assert!(matches!(
            two_args,
            PluginResult::Assertion(AssertionResult::Fail { .. })
        ));
    }

    #[test]
    #[cfg(not(miri))]
    fn arity_mismatch_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "one_arg.rhai", "fn one_arg(x) { x > 0 }");
        let plugins = load_trusted(&path).unwrap();
        let plugin = &plugins[0];

        let err = plugin
            .execute(&[Value::Number(1.into()), Value::Number(2.into())], &ctx())
            .unwrap_err();
        assert!(err.to_string().contains("called with 2 argument(s)"));
    }

    #[test]
    #[cfg(not(miri))]
    fn a_script_without_check_exposes_its_other_public_functions_instead() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "no_check.rhai", "fn not_check(x) { x }");
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "not_check");
    }

    #[test]
    #[cfg(not(miri))]
    fn a_file_with_no_public_functions_yields_no_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "empty.rhai", "private fn helper(x) { x }");
        let plugins = load_trusted(&path).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    #[cfg(not(miri))]
    fn rejects_a_script_that_fails_to_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(dir.path(), "syntax_error.rhai", "fn check(x) { {{{ ");
        assert!(load_trusted(&path).is_err());
    }

    #[test]
    #[cfg(not(miri))]
    fn one_file_can_define_many_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "validators.rhai",
            r#"
            fn is_even(x) { x % 2 == 0 }
            fn in_range(value, min, max) { value >= min && value <= max }
            private fn helper() { 1 }
            "#,
        );
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(plugins.len(), 2);
        assert_eq!(find(&plugins, "is_even").name(), "is_even");
        assert_eq!(find(&plugins, "in_range").name(), "in_range");
    }

    #[test]
    #[cfg(not(miri))]
    fn a_name_conflict_across_files_keeps_the_first_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        write_script(dir.path(), "a_first.rhai", "fn conflict_fn(x) { true }");
        write_script(dir.path(), "b_second.rhai", "fn conflict_fn(x) { false }");

        let typed = load_rhai_plugins_typed(dir.path());
        for p in &typed {
            let _ = p.trusted.set(true);
        }
        let plugins: Vec<std::sync::Arc<dyn Plugin>> = typed
            .into_iter()
            .map(|p| std::sync::Arc::new(p) as std::sync::Arc<dyn Plugin>)
            .collect();
        assert_eq!(plugins.len(), 1, "only the first definition should win");
        // "a_first.rhai" sorts before "b_second.rhai" — its `conflict_fn`
        // (always true) must be the one that survived.
        let result = plugins[0]
            .execute(&[Value::Number(1.into())], &ctx())
            .unwrap();
        assert!(matches!(
            result,
            PluginResult::Assertion(AssertionResult::Pass)
        ));
    }

    #[test]
    #[cfg(not(miri))]
    fn a_reporter_hook_is_not_also_registered_as_an_assertion_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "reporter_only.rhai",
            "fn on_test_end(name, result) { print(name); }",
        );
        let plugins = load_trusted(&path).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    #[cfg(not(miri))]
    fn doc_comment_becomes_the_plugin_description() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "documented.rhai",
            "/// Checks that a value is a positive number.\nfn check(x) { x > 0 }",
        );
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(
            plugins[0].description(),
            "Checks that a value is a positive number."
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn no_doc_comment_falls_back_to_the_generic_description() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "undocumented.rhai",
            "fn undocumented(x) { x > 0 }",
        );
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(plugins[0].description(), DEFAULT_DESCRIPTION);
    }

    #[test]
    #[cfg(not(miri))]
    fn param_and_return_type_tags_populate_the_signature() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "in_range.rhai",
            "/// Checks a value is within [min, max].\n\
             /// @param value: number\n\
             /// @param min: number\n\
             /// @param max: number\n\
             /// @returns bool\n\
             fn in_range(value, min, max) { value >= min && value <= max }",
        );
        let plugins = load_trusted(&path).unwrap();
        let sig = plugins[0].signature();
        assert_eq!(
            sig.arg_types.iter().map(|t| t.expected).collect::<Vec<_>>(),
            vec![TypeInfo::Number, TypeInfo::Number, TypeInfo::Number]
        );
        assert_eq!(sig.return_type, TypeInfo::Bool);
        assert_eq!(
            plugins[0].description(),
            "Checks a value is within [min, max]."
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn pure_tag_marks_the_plugin_safe_for_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "is_even.rhai",
            "/// @pure\nfn is_even(x) { x % 2 == 0 }",
        );
        let plugins = load_trusted(&path).unwrap();
        let sig = plugins[0].signature();
        assert_eq!(sig.purity, PluginPurity::Pure);
        assert!(sig.deterministic);
        assert!(sig.idempotent);
        assert!(sig.safe_for_rewrite);
    }

    #[test]
    #[cfg(not(miri))]
    fn without_pure_tag_stays_conservative_even_with_type_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "is_even.rhai",
            "/// @param x: number\n/// @returns bool\nfn is_even(x) { x % 2 == 0 }",
        );
        let plugins = load_trusted(&path).unwrap();
        let sig = plugins[0].signature();
        // Type tags and @pure are independent — no @pure means still Impure,
        // regardless of how precisely the types are declared.
        assert_eq!(sig.purity, PluginPurity::Impure);
        assert!(!sig.deterministic);
        assert!(!sig.safe_for_rewrite);
        assert_eq!(sig.return_type, TypeInfo::Bool);
    }

    #[test]
    #[cfg(not(miri))]
    fn untagged_params_default_to_any() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "mixed.rhai",
            "/// @param a: string\nfn mixed(a, b) { true }",
        );
        let plugins = load_trusted(&path).unwrap();
        let sig = plugins[0].signature();
        assert_eq!(
            sig.arg_types.iter().map(|t| t.expected).collect::<Vec<_>>(),
            vec![TypeInfo::String, TypeInfo::Any]
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn unrecognized_type_token_falls_back_to_any_without_erroring() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_script(
            dir.path(),
            "weird.rhai",
            "/// @returns nonsense_type\nfn weird(x) { true }",
        );
        let plugins = load_trusted(&path).unwrap();
        assert_eq!(plugins[0].signature().return_type, TypeInfo::Any);
    }

    #[test]
    #[cfg(not(miri))]
    fn load_rhai_plugins_skips_a_file_that_fails_to_compile_and_loads_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        write_script(dir.path(), "good.rhai", "fn good() { true }");
        write_script(dir.path(), "bad.rhai", "fn bad(x) { {{{ ");
        write_script(dir.path(), "not_a_plugin.txt", "ignored");

        let plugins = load_rhai_plugins(dir.path());
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "good");
    }

    #[test]
    #[cfg(not(miri))]
    fn load_rhai_plugins_on_missing_dir_returns_empty() {
        let plugins = load_rhai_plugins(Path::new("/nonexistent/plugin/dir"));
        assert!(plugins.is_empty());
    }
}
