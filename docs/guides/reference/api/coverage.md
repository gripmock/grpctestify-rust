# Coverage

`--coverage` reports how much of a proto API your test suite actually
exercises: which service methods were called, and which fields of the
request/response messages were covered by an assertion — against the *full*
descriptor pool (loaded the same way `reflect`/`scaffold --reflect` load it),
not just what shows up in your `.gctf` files.

```bash
grpctestify tests/ --coverage
grpctestify tests/ --coverage --coverage-format json
grpctestify tests/ --coverage --coverage-format html --log-output coverage.html
```

## Formats (`--coverage-format`)

- **text** (default) — a per-service method list and a field-coverage summary
  printed to the console alongside the normal test results.
- **json** — the same data as a structured report: per-file (service) stats,
  per-message field coverage, and overall summary/field_summary counts
  (`covered`/`total`) — for feeding into CI badges or other tooling.
- **html** — the same data as a single self-contained page (bar charts,
  service/method table, no CDN/JS charting library), in the same visual
  language as the [HTML test report](./report-formats#html).

## What counts as covered

- **Method coverage**: a method is covered if at least one test in the suite
  called it (an `ENDPOINT` targeting that service/method).
- **Field coverage**: a message field is covered if at least one `ASSERTS`
  expression in the suite references it. Field coverage is computed against
  fields your assertions *expect*, not fields the server actually returned in
  a given run — a field the server always omits but your assertions never
  check against will show as uncovered, which is the useful signal (it means
  nothing in your suite would catch that field going missing or changing
  shape).

## Requirements

Coverage reuses the same descriptor pool the run itself already resolved to
make its dynamic gRPC calls — whatever combination of reflection/`PROTO`
section/`--proto`/`--descriptor` your test files and CLI flags already use.
There's no separate coverage-specific descriptor lookup.
