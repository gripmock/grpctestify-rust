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
protocol: grpc-web
```

## Supported keys

- `timeout` - positive integer seconds
- `retry` - non-negative integer
- `retry_delay` - non-negative number
- `no_retry` - boolean
- `compression` - `none` or `gzip`
- `protocol` - `grpc` (default), `grpc-web` or `connectrpc`

## Wire protocol

`protocol` is what makes a test reproducible off the machine that wrote it. A document built against
`grpc-web` that records nothing about it runs over plain gRPC in CI, silently, and the failure looks
like a server problem rather than a transport mismatch.

```gctf
--- OPTIONS ---
protocol: grpc-web
```

- `--protocol` on the command line still wins, so a suite can be swept across transports without
  editing files.
- An unknown value is an error, not a fall back to `grpc` — a typo like `grpcweb` is exactly the
  mistaken run this key exists to prevent.
- When `ADDRESS` is absent, the default target follows the protocol
  (`localhost:4770` for gRPC, the protocol's own default otherwise).

## Rules

- Unknown keys produce validation warnings
- A key set twice is a parse error (not last-wins)
- Canonical keys use snake_case (`retry_delay`, `no_retry`)
- Runtime precedence quick map:
  - `run`: section attributes > `OPTIONS` > CLI runtime baseline/defaults
  - `bench`: CLI bench flags > `BENCH` section > bench defaults

## Related

- [Command Line](../api/command-line) (runtime and bench flags)
- [ATTRIBUTES](./attributes) (per-section runtime overrides)
- [BENCH](./bench) (`bench` model, separate precedence)
