// Reporter plugin: prints what each test *declared*, not what happened —
// `result.config_summary` (sections used, DATASET row count, TLS, PROTO
// files, chain step count) is a separate slice of `result` from the
// pass/fail outcome ndjson_metrics.rhai reports. Useful for spotting suite
// shape at a glance: "how many tests use TLS", "how many are chains".
//
// A field is only present when it has a non-default value (e.g. `tls` is
// absent, not `false`, when TLS wasn't configured) — `"key" in map` is the
// right way to check for it, not comparing against a default.
//
// Try it:
//   mkdir -p .grpctestify/plugins && cp test_shape_report.rhai .grpctestify/plugins/
//   grpctestify run tests/

fn on_test_end(name, result) {
    let cs = result.config_summary;
    let tls = if "tls" in cs { cs.tls } else { false };
    let dataset_rows = if "dataset_rows" in cs { cs.dataset_rows } else { 0 };
    print(`SHAPE ${name}: sections=${cs.sections} chain_steps=${cs.chain_steps} tls=${tls} dataset_rows=${dataset_rows}`);
}
