# Report Formats

`grpctestify` supports console output and file reports.

Use JSON for automation, JUnit for CI dashboards, and Allure for richer analytics.

## Console

```bash
grpctestify tests/
```

## JSON

```bash
grpctestify tests/ --log-format json --log-output results.json
```

## JUnit

```bash
grpctestify tests/ --log-format junit --log-output results.xml
```

## Allure

```bash
grpctestify tests/ --log-format allure --log-output allure-results
```

**Allure 2 and Allure 3 share the same `allure-results` on-disk contract** —
one result file per test plus a handful of sidecar files. grpctestify emits
that shared contract; there is no separate "Allure 2 mode" or version flag,
and the same `allure-results` directory works with either `allure generate`
(Allure 2 CLI) or Allure 3.

What's in `allure-results/`:

- `{uuid}-result.json` per test — status, timing, labels, per-assertion steps
  (expected/actual on failure), META (owner/tags/summary/links)
- `{uuid}-exchange-attachment.json` — the captured request/response headers,
  trailers, and body, attached to its test
- `{uuid}-container.json` per directory that has a `_setup.gctf`/
  `_teardown.gctf` fixture — links the fixture in as a before/after and the
  directory's ordinary tests as its children
- `categories.json` — a static defect taxonomy (assertion / gRPC status /
  connection / timeout / parse / validation) so failures group sensibly in
  the UI
- `environment.properties` — target address, grpctestify version, parallelism
- `executor.json` — populated automatically on GitHub Actions (or any CI
  setting the generic `CI`/`BUILD_URL`/`BUILD_NUMBER` variables)

Labels drive two navigation trees from the gRPC endpoint alone (no extra
config): **Suites** (`parentSuite`/`suite`/`subSuite` = package/service/method)
and **Behaviors** (`epic`/`feature`/`story` = the same). A test that only
passed after a retry is flagged `flaky` in its status details.

Allure attachments and per-assertion detail are only populated when the
runner captures the exchange — this happens automatically whenever
`--log-format allure` is set (or force it for any format with
`--capture-exchange`; see [Notes](#notes)).

## HTML

Single self-contained file, no network access required to view it (no CDN,
no JS at all — every interaction is native `<details>`/checkbox CSS): a hero
verdict banner leads with the pass/fail glyph, labeled Passed/Failed/Skipped/
Duration counts, and a thin ratio bar; failed tests get a red left-accent
border and stay expanded (every assertion, tags, owner, and a collapsible
captured exchange); the full test list collapses behind a summary when the
whole run is green, so a clean report stays short. Slowest tests/assertions
live in a secondary collapsed "Performance" section. Monospace type and a
light/dark toggle in the corner (a checkbox-driven CSS switch, no JS) tie the
report's look to the CLI's own terminal output.

```bash
grpctestify tests/ --log-format html --log-output report.html
```

## Multiple formats at once

`--log-format` accepts a comma-separated list to write several reports from
one run. With more than one format, `--log-output` becomes a directory
holding one file per format (`junit.xml`, `html.html`, an `allure/`
subdirectory, etc.) instead of an exact file path:

```bash
grpctestify tests/ --log-format junit,html --log-output reports/
```

## Notes

- Use `--log-format` together with `--log-output` to write file reports
- If `--log-output` is omitted, run continues and report file is skipped with a warning
- Set the `NO_COLOR` environment variable to disable colorized console output (no dedicated CLI flag)
- Reports can be combined with `--stream` for live integrations
- The captured request/response exchange is buffered automatically for
  verbose console output and for any format that renders it (Allure/JSON/
  YAML/HTML/JUnit) once `--log-output` is set; force it for any other case
  with `--capture-exchange`

## Metadata in reports

Reports include metadata from the `META` section:

- **JUnit**: `<property>` elements for tags/owner/summary/links, display name from `META.name`
- **Allure**: labels for tags, owner, package/service/method trees; `description` from `META.summary`; `links` from `META.links`
- **HTML**: tags and owner shown on each test's card
- **Console**: tags shown in error output
- **JSON / YAML**: full `meta` object in each test result

See [META](../sections/meta) and [Attributes](../sections/attributes) for details.
