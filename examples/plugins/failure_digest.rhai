// Reporter plugin: prints only failures, with their error message — useful
// on a large suite where the built-in console output is too noisy and you
// just want a short "what broke" digest at the end.
//
// Each hook call is independent (no state persists between calls) — this
// script only needs on_suite_end, since `results.results` already holds
// every test's full result, no need to accumulate across on_test_end calls.
//
// Try it:
//   mkdir -p .grpctestify/plugins && cp failure_digest.rhai .grpctestify/plugins/
//   grpctestify run tests/

fn on_suite_end(results) {
    let failures = results.results.filter(|r| r.status == "Fail");
    if failures.len() == 0 {
        print("DIGEST: no failures");
        return;
    }
    print(`DIGEST: ${failures.len()} failure(s)`);
    for r in failures {
        let reason = if "error_message" in r { r.error_message } else { "(no message)" };
        print(`  - ${r.name}: ${reason}`);
    }
}
