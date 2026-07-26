# Reporter Plugins (`.rhai` scripts)

A `.rhai` script under one of the [convention plugin directories](custom-scripts) that defines any of
`on_test_start`/`on_test_end`/`on_suite_end` becomes an extra reporter for `run`, receiving the same data
every built-in reporter (JSON, JUnit, ...) sees.

```rhai
// .grpctestify/plugins/metrics.rhai
fn on_test_end(name, result) {
    print(`METRIC test=${name} status=${result.status} duration_ms=${result.duration_ms}`);
}

fn on_suite_end(results) {
    print(`METRIC suite total=${results.total} passed=${results.passed} failed=${results.failed}`);
}
```

## What each hook receives

`result` (in `on_test_end`) is a single test's result — the same shape as the JSON reporter's output. Its main fields:

- `status` — `"Pass"`, `"Fail"`, or `"Skip"`
- `duration_ms`, `call_duration_ms` — total and on-the-wire timing
- `error_message` — failure detail, when failed
- `meta` — `name`, `summary`, `tags`, `owner`, `links`
- `assertions`, `retried`
- `config_summary` — what the test *declared*: `sections` used, `dataset_rows`, `tls`, `proto_files`,
  `chain_steps`. A field appears only when it has a non-default value (e.g. `tls` is absent, not `false`,
  when TLS wasn't configured).

`results` (in `on_suite_end`) is the full suite: `total`/`passed`/`failed`/`skipped` counts, plus
`results` — the array of every test's result.

## Constraints

- The only output channel is Rhai's built-in `print`/`debug` (stdout/stderr) — no file or network access is
  granted, same as assertion plugins. Pipe stdout to whatever collects your metrics downstream.
- Each hook call is independent — no state persists between calls. For a running total, compute it from the
  full `results` array in `on_suite_end` rather than accumulating across `on_test_end` calls.
- A script can define both assertion-plugin functions and a reporter hook, or just one — detection is
  per-function, not per-file-location. See [Custom assertion plugins](custom-scripts).
- A throwing/erroring hook is logged and skipped; it does not abort the run.

## Related

- [Plugin System overview](index)
- [Custom assertion plugins](custom-scripts)
- [Installing from a git host](installing)
- [Script stdlib](stdlib)
