# Basic Concepts

Core rules for writing and running `.gctf` tests.

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

- `META` - file-level metadata (name, summary, tags, owner, links); if present, must be the first section
- `ADDRESS` - target host and port
- `ENDPOINT` - gRPC method in `package.Service/Method` format
- `REQUEST` - JSON payload (multiple allowed)
- `RESPONSE` - expected JSON payload (multiple allowed)
- `ERROR` - expected error payload
- `ASSERTS` - validation expressions
- `EXTRACT` - extract values for reuse and advanced checks
- `REQUEST_HEADERS` - request metadata
- `TLS` - TLS/mTLS parameters
- `PROTO` - descriptor/reflection configuration
- `OPTIONS` - parsed, validated, and used for per-test runtime overrides
  (`timeout`, `retry`, `retry_delay`, `no_retry`, `compression`)

Use [Section Reference](../reference/sections/) for exact syntax and section rules.

## Required rules

- `META` is optional, but only one is allowed and it must be first
- `ENDPOINT` is required
- At least one validation block is required: `RESPONSE`, `ERROR`, or `ASSERTS`
- `RESPONSE` and `ERROR` cannot be used in the same test file
- `ADDRESS` can be omitted if `GRPCTESTIFY_ADDRESS` is set

## META example

```gctf
--- META ---
name: Say hello returns greeting
summary: Smoke check for unary hello endpoint
tags: [smoke, hello]
owner: qa-platform
links:
  - https://github.com/gripmock/grpctestify-rust
```

## RPC patterns

- Unary: one `REQUEST` + one `RESPONSE`
- Client streaming: multiple `REQUEST` + one `RESPONSE`
- Server streaming: one `REQUEST` + multiple `RESPONSE` or `ASSERTS`
- Bidirectional streaming: alternating `REQUEST` and checks

## Assertion examples

```gctf
--- ASSERTS ---
.status == "ok"
@len(.items) > 0
@header("x-request-id") != null
@uuid(.user.id)
```

## Inline response options

Use inline options in the section header when matching needs tuning:

```gctf
--- RESPONSE with_asserts partial tolerance=0.1 unordered_arrays ---
{
  "price": 9.99,
  "tags": ["a", "b"]
}
```

- `with_asserts` - allow additional `ASSERTS` checks after payload matching
- `partial` - allow expected payload to be a subset of actual payload
- `tolerance` - numeric tolerance for floating-point comparisons
- `unordered_arrays` - ignore array element ordering during comparison

## Runtime configuration

Use CLI flags for runtime behavior:

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
