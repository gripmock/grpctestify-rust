# REQUEST_HEADERS

Request metadata sent with RPC calls.

## When to use

- Add auth tokens, API keys, and trace IDs
- Validate metadata behavior together with `@header()` assertions

## Minimal example

```gctf
--- REQUEST_HEADERS ---
authorization: Bearer test-token
x-request-id: req-123
```

## Rules

- Legacy `HEADERS` alias is recognized but deprecated
- One section can include multiple key-value pairs
- The same header name can't be set twice (a parse error) — each key maps to one value, not a multi-value list

## Related

- [ASSERTS section](./asserts)
