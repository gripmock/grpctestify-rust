# Testing Patterns

## RPC modes

- Unary: one `REQUEST` and one `RESPONSE` or `ASSERTS`
- Client streaming: multiple `REQUEST`, one `RESPONSE`
- Server streaming: one `REQUEST`, multiple `RESPONSE` or `ASSERTS`
- Bidirectional streaming: alternating `REQUEST` and validation blocks

## Recommended docs

- [Data Validation](data-validation)
- [Error Testing](error-testing)
- [Security Testing](security-testing)
- [Performance Testing](performance-testing)
- [Assertion Patterns](assertion-patterns)

## Runtime flags

```bash
grpctestify tests/ --parallel 4 --timeout 30
grpctestify tests/ --log-format json --log-output results.json
```

`OPTIONS` blocks support per-test overrides for `timeout`, `retry`, `retry-delay`, and `no-retry`.
