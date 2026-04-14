# RESPONSE

Expected successful response payload.

## When to use

- Use for strict contract validation
- Can appear multiple times for streaming expectations
- Combine with `with_asserts` when you need payload + logical checks

## Minimal example

```gctf
--- RESPONSE with_asserts partial tolerance=0.1 unordered_arrays ---
{
  "status": "ok"
}
```

## Inline options

- `with_asserts`
- `partial`
- `tolerance=<number>`
- `redact=["field1","field2"]`
- `unordered_arrays`

## Rules

- Do not combine `RESPONSE` and `ERROR` in one file

## Related

- [Assertions](../api/assertions)
- [ERROR section](./error)
