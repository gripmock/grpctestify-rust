// A single script can be both an assertion plugin AND a reporter — which
// role(s) it plays is decided purely by which functions it defines, not by
// where the file lives.
//
// Assertion plugin: @combined_example(value) — checks the value is a
// non-empty string.
// Reporter plugin: on_test_start prints a small "starting" marker.
//
// Usage:
//   ASSERTS: @combined_example(.name)
//   mkdir -p .grpctestify/plugins && cp combined_example.rhai .grpctestify/plugins/
//   grpctestify run

fn combined_example(value) {
    type_of(value) == "string" && value.len() > 0
}

fn on_test_start(name) {
    print(`-> starting ${name}`);
}
