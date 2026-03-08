# Type Validation

Built-in validators for common formats.

## Supported validators

- `@uuid(value, "v1"|"v4"|"v5"|"any")`
- `@timestamp(value, "iso8601"|"rfc3339"|"unix"|"unix_ms")`
- `@url(value, "any"|"http"|"https"|"ws"|"wss"|"ftp")`
- `@email(value, "strict")`
- `@ip(value, "v4"|"v6")`

## Examples

```gctf
--- ASSERTS ---
@uuid(.user.id, "v4")
@timestamp(.created_at, "rfc3339")
@url(.website, "https")
@email(.contact.email)
@ip(.client_ip, "v4")
```
