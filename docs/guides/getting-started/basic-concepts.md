# Basic Concepts

Core concepts for writing and running `.gctf` tests.

## What a test contains

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
package.Service/Method

--- REQUEST ---
{
  "id": 1
}

--- ASSERTS ---
.id == 1
```

## Main sections

- `ADDRESS` - target host and port
- `ENDPOINT` - gRPC method in `package.Service/Method` format
- `REQUEST` - JSON payload (multiple allowed)
- `RESPONSE` - expected JSON payload (multiple allowed)
- `ERROR` - expected error payload
- `ASSERTS` - validation expressions
- `REQUEST_HEADERS` - request metadata
- `TLS` - TLS/mTLS parameters
- `PROTO` - descriptor/reflection configuration
- `OPTIONS` - parsed, validated, and used for per-test runtime overrides (`timeout`, `retry`, `retry-delay`, `no-retry`)

## RPC patterns

- Unary: one `REQUEST` + one `RESPONSE`
- Client streaming: multiple `REQUEST` + one `RESPONSE`
- Server streaming: one `REQUEST` + multiple `RESPONSE` or `ASSERTS`
- Bidirectional streaming: alternating `REQUEST` and validations

## Assertion examples

```gctf
--- ASSERTS ---
.status == "ok"
.items | length > 0
@header("x-request-id") != null
@uuid(.user.id, "v4")
```

## Runtime configuration

Use CLI flags for execution behavior:

```bash
grpctestify tests/ --parallel 4 --timeout 30
grpctestify tests/ --log-format json --log-output results.json
```

Useful environment variables:

```bash
export GRPCTESTIFY_ADDRESS="localhost:4770"
export GRPCTESTIFY_COMPRESSION="gzip"
export GRPCTESTIFY_TLS_CA_FILE="./certs/ca.pem"
```
