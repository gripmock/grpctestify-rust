# Script Stdlib

Every script engine — [assertion plugin](custom-scripts) or [reporter](reporters) — gets the same small set of native helpers registered automatically, no import needed:

| Function | Does |
| --- | --- |
| `log_info(msg)` / `log_warn(msg)` / `log_error(msg)` / `log_debug(msg)` | Routes through grpctestify's own `tracing` output — respects `--verbose`/log filtering, unlike Rhai's raw `print`/`debug` which always writes straight to stdout/stderr regardless of CLI flags |
| `is_uuid(s)` / `is_email(s)` / `is_ip(s)` / `is_url(s)` / `is_timestamp(s)` | `bool` — the exact same validators backing `@is_uuid`/`@is_email`/`@is_ip`/`@is_url`/`@is_timestamp` |
| `regex_match(s, pattern)` | `bool` — Rhai has no regex support on its own; shares the same compiled-pattern cache the built-in `@regex` plugin uses |

```rhai
// plugins/stdlib_demo.rhai
fn stdlib_demo(value) {
    if !is_uuid(value) {
        log_warn(`not a UUID: ${value}`);
        return false;
    }
    true
}
```

This keeps a script from reimplementing what already exists (see how `luhn_valid.rhai`/`is_palindrome.rhai` had to hand-roll their logic before this existed) — it's a small, deliberately curated set, not a mirror of every built-in plugin. All of it stays inside the pure-function boundary: no file or network access is granted by any of these helpers.

## Related

- [Plugin System overview](index)
- [Custom assertion plugins](custom-scripts)
- [Reporter plugins](reporters)
