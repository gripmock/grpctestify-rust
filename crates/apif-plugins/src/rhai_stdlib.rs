//! Native helpers registered into every `.rhai` script engine — both
//! assertion plugins (`RhaiPlugin`) and reporters (`RhaiReporter`) call
//! [`build_engine`] so the two don't drift, either in what's registered or
//! in how the engine is sandboxed.
//!
//! Two kinds of helper, both deliberately small:
//!
//! - `log_info`/`log_warn`/`log_error`/`log_debug` route through
//!   grpctestify's own `tracing` output instead of Rhai's raw `print`/
//!   `debug` (which always writes straight to stdout/stderr, ignoring
//!   `--verbose`/log filtering entirely).
//! - `is_uuid`/`is_email`/`is_ip`/`is_url`/`is_timestamp`/`regex_match` wrap
//!   the exact same crates backing the built-in `@is_uuid`/`@regex`/...
//!   plugins, so a custom script can reuse them instead of hand-rolling
//!   validation logic (as the first `luhn_valid.rhai`/`is_palindrome.rhai`
//!   examples had to before this existed).
//!
//! All of it stays inside the pure-function boundary already documented for
//! script plugins: no file or network access, nothing added here changes
//! that.
//!
//! # Sandboxing
//!
//! `Engine::new()` on its own does *not* bound operation count or
//! string/array/map size (Rhai's own defaults leave those unlimited) — a
//! buggy or malicious script (`while true {}`, a huge string literal) would
//! otherwise hang or OOM the whole `run` process, not just itself.
//! [`build_engine`] sets generous-but-finite limits and disables Rhai's
//! built-in `eval` (dynamic string-as-code execution has no legitimate use
//! in a validator/reporter script and is an easy footgun if a script ever
//! passes response data into it). Call/expression nesting depth already has
//! sane built-in Rhai defaults and isn't touched here.

use rhai::Engine;

/// Operation count ceiling — generous for any realistic validator/reporter
/// (a Luhn-checksum loop over a card number is a few dozen operations), but
/// stops a runaway loop within a fraction of a second instead of hanging
/// the whole `run` process.
const MAX_OPERATIONS: u64 = 1_000_000;
/// Per-string size ceiling — comfortably larger than any realistic gRPC
/// field, small enough to block a multi-gigabyte string-literal bomb.
const MAX_STRING_SIZE: usize = 10_000_000;
/// Per-array/map element-count ceiling, same rationale as the string limit.
const MAX_COLLECTION_SIZE: usize = 100_000;

/// Build a `.rhai` script engine with the shared stdlib registered and the
/// sandbox limits applied — the only way either `RhaiPlugin` or
/// `RhaiReporter` should construct an `Engine`.
pub fn build_engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .set_max_operations(MAX_OPERATIONS)
        .set_max_string_size(MAX_STRING_SIZE)
        .set_max_array_size(MAX_COLLECTION_SIZE)
        .set_max_map_size(MAX_COLLECTION_SIZE)
        .disable_symbol("eval");
    // `Engine::new()` ships a filesystem resolver, so `import "../../x"` both
    // escaped the plugin dir and ran the imported script's top level.
    engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    register(&mut engine);
    engine
}

/// Read a script, hash it, and compile it — in that order, from one buffer.
///
/// The trust gate compares the stored digest against the script that is about
/// to run, so the digest must cover the exact bytes that were compiled. Both
/// `RhaiPlugin::load_all` and `RhaiReporter::load` need this, and having them
/// each spell it out invited the two copies to drift apart; the invariant lives
/// here instead.
pub fn compile_with_digest(
    engine: &Engine,
    path: &std::path::Path,
) -> anyhow::Result<(rhai::AST, String)> {
    use anyhow::Context;

    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read plugin script: {}", path.display()))?;
    let digest = crate::marketplace::sha256_hex(&bytes);
    let source = String::from_utf8(bytes)
        .with_context(|| format!("plugin script is not UTF-8: {}", path.display()))?;
    let ast = engine
        .compile(&source)
        .with_context(|| format!("failed to compile plugin script: {}", path.display()))?;
    Ok((ast, digest))
}

fn register(engine: &mut Engine) {
    engine.register_fn("log_info", |msg: &str| tracing::info!("{msg}"));
    engine.register_fn("log_warn", |msg: &str| tracing::warn!("{msg}"));
    engine.register_fn("log_error", |msg: &str| tracing::error!("{msg}"));
    engine.register_fn("log_debug", |msg: &str| tracing::debug!("{msg}"));

    engine.register_fn("is_uuid", |s: &str| uuid::Uuid::parse_str(s).is_ok());
    engine.register_fn("is_email", |s: &str| {
        email_address::EmailAddress::is_valid(s)
    });
    engine.register_fn("is_ip", |s: &str| s.parse::<std::net::IpAddr>().is_ok());
    engine.register_fn("is_url", |s: &str| url::Url::parse(s).is_ok());
    engine.register_fn("is_timestamp", |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s).is_ok()
    });
    engine.register_fn("regex_match", |s: &str, pattern: &str| -> bool {
        apif_assert::cached_regex(pattern)
            .map(|re| re.is_match(s))
            .unwrap_or(false)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_stdlib() -> Engine {
        build_engine()
    }

    #[test]
    fn runaway_loop_is_stopped_by_the_operation_limit() {
        let engine = engine_with_stdlib();
        let err = engine
            .eval::<i64>("let x = 0; while true { x += 1; } x")
            .unwrap_err();
        assert!(
            err.to_string()
                .to_lowercase()
                .contains("too many operations"),
            "expected an operation-limit error, got: {err}"
        );
    }

    /// The trust gate is only sound if the digest covers the bytes that were
    /// actually compiled — pin that the returned digest is the file's own
    /// hash, not something computed from a re-read or a re-encode.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn compile_with_digest_hashes_exactly_what_it_compiled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.rhai");
        let source = b"fn check(value) { value == 1 }";
        std::fs::write(&path, source).unwrap();

        let engine = build_engine();
        let (ast, digest) = compile_with_digest(&engine, &path).unwrap();

        assert_eq!(digest, crate::marketplace::sha256_hex(source));
        assert!(ast.iter_functions().any(|f| f.name == "check"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn compile_with_digest_rejects_a_script_that_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.rhai");
        std::fs::write(&path, b"fn check(value) { value == }").unwrap();

        let engine = build_engine();
        assert!(compile_with_digest(&engine, &path).is_err());
    }

    #[test]
    fn script_level_eval_is_disabled() {
        let engine = engine_with_stdlib();
        let err = engine.eval::<i64>(r#"eval("1 + 1")"#).unwrap_err();
        assert!(
            err.to_string().contains("eval"),
            "expected eval to be disabled, got: {err}"
        );
    }

    #[test]
    fn a_normal_check_plugin_still_runs_well_under_the_limits() {
        // The whole point of generous limits: real validator logic (a
        // digit-summing loop, same shape as the Luhn checksum example) must
        // not be anywhere near them.
        let engine = engine_with_stdlib();
        let result: bool = engine
            .eval(
                r#"
                let digits = "1234567890";
                let sum = 0;
                let i = digits.len() - 1;
                while i >= 0 {
                    sum += digits[i].to_int() - '0'.to_int();
                    i -= 1;
                }
                sum == 45
                "#,
            )
            .unwrap();
        assert!(result);
    }

    #[test]
    fn is_uuid_validates() {
        let engine = engine_with_stdlib();
        assert!(
            engine
                .eval::<bool>(r#"is_uuid("550e8400-e29b-41d4-a716-446655440000")"#)
                .unwrap()
        );
        assert!(!engine.eval::<bool>(r#"is_uuid("not-a-uuid")"#).unwrap());
    }

    #[test]
    fn is_email_validates() {
        let engine = engine_with_stdlib();
        assert!(engine.eval::<bool>(r#"is_email("a@example.com")"#).unwrap());
        assert!(!engine.eval::<bool>(r#"is_email("nope")"#).unwrap());
    }

    #[test]
    fn is_ip_validates() {
        let engine = engine_with_stdlib();
        assert!(engine.eval::<bool>(r#"is_ip("127.0.0.1")"#).unwrap());
        assert!(!engine.eval::<bool>(r#"is_ip("not-an-ip")"#).unwrap());
    }

    #[test]
    fn is_url_validates() {
        let engine = engine_with_stdlib();
        assert!(
            engine
                .eval::<bool>(r#"is_url("https://example.com")"#)
                .unwrap()
        );
        assert!(!engine.eval::<bool>(r#"is_url("not a url")"#).unwrap());
    }

    #[test]
    fn is_timestamp_validates() {
        let engine = engine_with_stdlib();
        assert!(
            engine
                .eval::<bool>(r#"is_timestamp("2026-07-23T10:00:00Z")"#)
                .unwrap()
        );
        assert!(
            !engine
                .eval::<bool>(r#"is_timestamp("not-a-timestamp")"#)
                .unwrap()
        );
    }

    #[test]
    fn regex_match_uses_the_shared_cache() {
        let engine = engine_with_stdlib();
        assert!(
            engine
                .eval::<bool>(r#"regex_match("hello123", "^[a-z]+\\d+$")"#)
                .unwrap()
        );
        assert!(
            !engine
                .eval::<bool>(r#"regex_match("HELLO", "^[a-z]+$")"#)
                .unwrap()
        );
    }

    #[test]
    fn regex_match_with_a_bad_pattern_fails_closed() {
        let engine = engine_with_stdlib();
        assert!(
            !engine
                .eval::<bool>(r#"regex_match("x", "(unclosed")"#)
                .unwrap()
        );
    }

    #[test]
    fn log_functions_do_not_error() {
        let engine = engine_with_stdlib();
        engine
            .eval::<()>(
                r#"
                log_info("info line");
                log_warn("warn line");
                log_error("error line");
                log_debug("debug line");
                "#,
            )
            .unwrap();
    }

    #[test]
    fn a_check_plugin_can_call_the_stdlib() {
        let engine = engine_with_stdlib();
        let ast = engine
            .compile(r#"fn check(value) { is_uuid(value) }"#)
            .unwrap();
        let result: bool = engine
            .call_fn(
                &mut rhai::Scope::new(),
                &ast,
                "check",
                ("550e8400-e29b-41d4-a716-446655440000",),
            )
            .unwrap();
        assert!(result);
    }
}
