# Custom Plugin Examples (`.rhai`)

Copy any of these into `~/.grpctestify/plugins` or `./.grpctestify/plugins` and they're loaded automatically — no flag. Every public function in a script becomes its own assertion plugin, named directly after itself (`fn is_even(x) {...}` → `@is_even(...)`) — one file can define many. `on_test_start`/`on_test_end`/`on_suite_end` make a script a reporter instead (or as well). `private fn` stays internal. Full reference: [docs/guides/plugins](../../docs/guides/plugins/).

```bash
mkdir -p .grpctestify/plugins
cp examples/plugins/*.rhai .grpctestify/plugins/
grpctestify run tests/
```

## Assertion plugins

| Script | Call | Checks | Demonstrates |
| --- | --- | --- | --- |
| `validators.rhai` | `@is_even(.count)`, `@in_range(.age, 18, 65)` | numeric parity, value within `[min, max]` | one file, two plugins; `@param`/`@returns`/`@pure` type tags |
| `string_utils.rhai` | `@is_palindrome(.code)`, `@slugify(.title) == "hello-world"` | palindrome check; value-returning (not bool), composes with `==` | `private fn` helper hidden from export |
| `luhn_valid.rhai` | `@luhn_valid(.card_number)` | Luhn checksum (card numbers, IMEI, ...) | non-trivial real logic, `@pure` |
| `flexible_match.rhai` | `@flexible_match(.id)` / `@flexible_match(.code, "^[A-Z]{3}$")` | present; present AND matches a pattern | arity overloading — one name, two parameter counts |
| `stdlib_demo.rhai` | `@stdlib_demo(.request_id)` | UUID + hex-pattern check | `is_uuid`/`regex_match`/`log_*` stdlib helpers |

## Reporter plugins

| Script | Hook(s) | Does |
| --- | --- | --- |
| `slow_test_alert.rhai` | `on_test_end` | flags any test over 200ms |
| `ndjson_metrics.rhai` | `on_test_end`, `on_suite_end` | one JSON line per test + a summary line |
| `failure_digest.rhai` | `on_suite_end` | prints only failures, with their error message |
| `test_shape_report.rhai` | `on_test_end` | prints what each test *declared* (`config_summary`: sections, TLS, chain steps, DATASET rows) — not pass/fail |

## Both at once

`combined_example.rhai` defines `combined_example` (assertion) and `on_test_start` (reporter) in the same file — a script isn't limited to one role, and one file isn't limited to one plugin.
