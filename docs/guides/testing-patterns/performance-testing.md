# Performance Testing

Performance testing in gRPC Testify focuses on timeout control, parallel execution, and metrics in reports.

## What to Use

- `--timeout` for per-test deadlines
- `--parallel` for concurrency
- `--log-format` + `--log-output` for machine-readable metrics
- `--coverage` for API-level execution coverage

## Timeout Examples

```bash
# Run with stricter timeout
grpctestify tests/ --timeout 10

# Run one test with extended timeout
grpctestify slow_case.gctf --timeout 60
```

## Parallel Execution

```bash
# Auto detect worker count
grpctestify tests/ --parallel auto

# Fixed workers
grpctestify tests/ --parallel 4
```

## Reporting for Analysis

```bash
# JSON report
grpctestify tests/ --log-format json --log-output perf-results.json

# JUnit report
grpctestify tests/ --log-format junit --log-output perf-results.xml
```

## Notes

- Use CLI flags for execution tuning
- `OPTIONS` section is parsed but not currently applied at runtime
