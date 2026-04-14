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

## Related

- [ASSERTS section](./asserts)
