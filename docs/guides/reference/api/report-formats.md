# Report Formats

`grpctestify` supports console output and file reports.

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

- `--log-format` requires `--log-output`
- Use `--no-color` in CI logs when needed
