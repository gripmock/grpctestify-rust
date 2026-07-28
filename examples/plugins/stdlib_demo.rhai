// Assertion plugin: @stdlib_demo(value)
//
// Every .rhai script (assertion plugin or reporter) gets a small set of
// native helpers for free — no import needed:
//   log_info/log_warn/log_error/log_debug(msg)  - routed through
//     grpctestify's own tracing output (respects --verbose/log filtering),
//     unlike Rhai's raw print/debug which always writes straight to
//     stdout/stderr.
//   is_uuid/is_email/is_ip/is_url/is_timestamp(s) -> bool - the exact same
//     validators backing the built-in @is_uuid/@is_email/... plugins.
//   regex_match(s, pattern) -> bool - Rhai has no regex support on its own;
//     this shares the same compiled-pattern cache the built-in @regex
//     plugin uses.
//
// Usage in ASSERTS:
//   @stdlib_demo(.request_id)

/// Checks a value is a UUID that also looks hex/dash-only, logging why on
/// failure. Not tagged @pure — it calls log_warn/log_debug, and side
/// effects (even harmless ones like logging) aren't something to casually
/// mix with the "safe to let the optimizer rewrite around this" claim.
/// @param value: string
/// @returns bool
fn stdlib_demo(value) {
    if !is_uuid(value) {
        log_warn(`stdlib_demo: "${value}" is not a UUID`);
        return false;
    }
    log_debug(`stdlib_demo: "${value}" is a valid UUID`);
    regex_match(value, "^[0-9a-f-]+$")
}
