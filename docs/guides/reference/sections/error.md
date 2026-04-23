# ERROR

Expected failed call result.

## When to use

- Use when RPC is expected to fail
- Use `partial` to match only a subset of error fields
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

- `ERROR` supports `partial` and `with_asserts`
- In strict mode (default), missing `message` in expected fails if server returns it
- `details` may be omitted only when server does not return `details`
- `with_asserts` must be followed immediately by `ASSERTS`
- Empty `ERROR with_asserts` before `ASSERTS` is accepted but warned as redundant (prefer standalone `ASSERTS`)
- Do not combine `ERROR` and `RESPONSE` in one file

## Related

- [Assertions](../api/assertions)
- [RESPONSE section](./response)
