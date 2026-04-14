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

## Notes

- Use `--log-format` together with `--log-output` to write file reports
- If `--log-output` is omitted, run continues and report file is skipped with a warning
- Use `--no-color` in CI logs if needed
- Reports can be combined with `--stream` for live integrations
