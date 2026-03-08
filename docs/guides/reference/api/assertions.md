# Assertions

Use `ASSERTS` to validate responses.

## Basic examples

```gctf
--- ASSERTS ---
.status == "ok"
.count > 0
.items | length > 0
.user.email | test("@")
```

## Metadata helpers

```gctf
--- ASSERTS ---
@header("x-request-id") != null
@trailer("x-processing-time") != null
```

## Type helpers

```gctf
--- ASSERTS ---
@uuid(.user.id, "v4")
@email(.user.email)
@url(.profile.website, "https")
@ip(.client_ip, "v4")
@timestamp(.created_at, "rfc3339")
```

## Notes

- `ASSERTS` can be used alone or together with `RESPONSE with_asserts=true`
- For unary tests, prefer either strict `RESPONSE` matching or `ASSERTS`

## Preferred style

- Use boolean plugin calls directly: `@has_header("x-id")` instead of `@has_header("x-id") == true`
- Use negation for false checks: `!@has_trailer("grpc-status-details-bin")`
- Use canonical operators: `startsWith` and `endsWith`
