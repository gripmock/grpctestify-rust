# OPTIONS

Per-test runtime overrides.

## When to use

- Tune one test without changing global CLI flags
- Set timeout/retry behavior near the scenario

## Minimal example

```gctf
--- OPTIONS ---
timeout: 60
retry: 2
retry_delay: 1.5
no_retry: false
compression: gzip
```

## Supported keys

- `timeout` - positive integer seconds
- `retry` - non-negative integer
- `retry_delay` - non-negative number
- `no_retry` - boolean
- `compression` - `none` or `gzip`

## Rules

- Unknown keys produce validation warnings
- Canonical keys use snake_case (`retry_delay`, `no_retry`)
- Runtime precedence quick map:
  - `run`: section attributes > `OPTIONS` > CLI runtime baseline/defaults
  - `bench`: CLI bench flags > `BENCH` section > bench defaults

## Related

- [Command Line](../api/command-line) (runtime and bench flags)
- [ATTRIBUTES](./attributes) (per-section runtime overrides)
- [BENCH](./bench) (`bench` model, separate precedence)
