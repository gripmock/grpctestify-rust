use rhai::Engine;

const MAX_OPERATIONS: u64 = 1_000_000;
const MAX_STRING_SIZE: usize = 10_000_000;
const MAX_COLLECTION_SIZE: usize = 100_000;

pub fn build_engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .set_max_operations(MAX_OPERATIONS)
        .set_max_string_size(MAX_STRING_SIZE)
        .set_max_array_size(MAX_COLLECTION_SIZE)
        .set_max_map_size(MAX_COLLECTION_SIZE)
        .disable_symbol("eval");
    engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    register(&mut engine);
    engine
}

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
