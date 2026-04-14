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
retry-delay: 1.5
no-retry: false
compression: gzip
```

## Supported keys

- `timeout` - positive integer seconds
- `retry` - non-negative integer
- `retry-delay` - non-negative number
- `no-retry` - boolean
- `compression` - `none` or `gzip`

## Rules

- Unknown keys produce validation warnings

## Related

- [Command Line](../api/command-line)
