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

## Timing helpers

Timing helpers are available inside `ASSERTS` and are most useful with `RESPONSE with_asserts=true`:

```gctf
--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
@elapsed_ms() >= 10
@total_elapsed_ms() >= 10
```

- `@elapsed_ms()` - elapsed for current assertion scope.
- `@total_elapsed_ms()` - cumulative elapsed across completed assertion scopes.
- `@scope_message_count()` - number of response messages in current scope.
- `@scope_index()` - current scope index (1-based).

Scope behavior:

- Single message in `RESPONSE` section -> single-message scope.
- Multiple messages in one `RESPONSE` section -> batch scope for the whole section.

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
