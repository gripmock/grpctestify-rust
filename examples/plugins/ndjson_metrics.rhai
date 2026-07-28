// Reporter plugin: emits one NDJSON line per test, plus a summary line —
// pipe stdout into whatever collects your metrics downstream (a log
// aggregator, jq, a file). print()/debug() (stdout/stderr) are the only
// output channel a script gets — no file/network access, same pure-function
// boundary as assertion plugins.
//
// Try it:
//   mkdir -p .grpctestify/plugins && cp ndjson_metrics.rhai .grpctestify/plugins/
//   grpctestify run tests/ | grep '"metric":"test"'

fn on_test_end(name, result) {
    print(`{"metric":"test","name":"${name}","status":"${result.status}","duration_ms":${result.duration_ms}}`);
}

fn on_suite_end(results) {
    print(`{"metric":"suite","total":${results.total},"passed":${results.passed},"failed":${results.failed},"skipped":${results.skipped}}`);
}
