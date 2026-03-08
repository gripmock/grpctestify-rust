# Data Validation

Use `RESPONSE` for strict matching or `ASSERTS` for flexible checks.

## Strict response matching

```gctf
--- RESPONSE ---
{
  "status": "ok",
  "user": { "id": "123" }
}
```

## Flexible assertions

```gctf
--- ASSERTS ---
.status == "ok"
.user.id | type == "string"
.items | length > 0
```

## Inline comparison options

```gctf
--- RESPONSE partial=true tolerance=0.1 unordered_arrays=true ---
{
  "price": 9.99,
  "tags": ["a", "b"]
}
```

Notes:

- `OPTIONS` section supports runtime overrides (`timeout`, `retry`, `retry-delay`, `no-retry`)
- Use `RESPONSE with_asserts=true` when you need both response match and assertions
