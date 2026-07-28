// Reporter plugin: flags slow tests as they finish.
//
// A script becomes a reporter simply by defining on_test_start/on_test_end/
// on_suite_end — same convention plugin directory as assertion plugins.
// `result` carries the same fields the JSON reporter writes (status,
// duration_ms, meta, ...).
//
// Try it:
//   mkdir -p .grpctestify/plugins && cp slow_test_alert.rhai .grpctestify/plugins/
//   grpctestify run tests/

// rhai functions don't see script-level `const`/`let` (each has its own
// isolated scope) — a helper function is the way to share a constant across
// functions in the same script. `private fn` (not `fn`) keeps it from also
// being exposed as its own assertion plugin — every public function in a
// plugin script becomes callable as `@<name>(...)`.
private fn slow_threshold_ms() {
    200
}

fn on_test_end(name, result) {
    let threshold = slow_threshold_ms();
    if result.duration_ms > threshold {
        print(`SLOW (${result.duration_ms}ms > ${threshold}ms): ${name}`);
    }
}
