# ERROR

Expected failed call result.

## When to use

- Use when RPC is expected to fail
- Add `with_asserts` for extra checks after error matching

## Minimal example

```gctf
--- ERROR with_asserts ---
{
  "code": 3,
  "message": "Invalid input"
}

--- ASSERTS ---
.code == 3
.message != null
```

## Rules

- `ERROR` supports `with_asserts` only
- Do not combine `ERROR` and `RESPONSE` in one file

## Related

- [Assertions](../api/assertions)
- [RESPONSE section](./response)
