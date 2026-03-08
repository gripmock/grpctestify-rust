# Core gRPC Execution

This page describes built-in test execution behavior in Rust CLI.

## What is built in

- gRPC transport and request execution
- unary and streaming call handling
- JSON request/response validation
- assertion execution
- TLS/mTLS support
- descriptor loading via reflection or `PROTO descriptor=<path>`

## Basic run example

```gctf
--- ADDRESS ---
localhost:9090

--- ENDPOINT ---
user.UserService/GetUser

--- REQUEST ---
{ "user_id": "123" }

--- RESPONSE ---
{ "user_id": "123" }
```

## Runtime controls

```bash
grpctestify tests/ --timeout 30
grpctestify tests/ --parallel 4
grpctestify tests/ --verbose
```

## RESPONSE inline options

```gctf
--- RESPONSE partial=true tolerance=0.1 unordered_arrays=true ---
{
  "status": "ok"
}
```

## Notes

- `OPTIONS` section can override per-test runtime flags (`timeout`, `retry`, `retry-delay`, `no-retry`)
- `PROTO files=...` is not supported in native mode
