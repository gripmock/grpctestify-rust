# Plugin System

Rust version includes built-in assertion plugins available in `ASSERTS`.

## Available Plugins

- `@header("name")`
- `@has_header("name")`
- `@trailer("name")`
- `@has_trailer("name")`
- `@uuid(value, "v4")`
- `@email(value)`
- `@ip(value, "v4"|"v6")`
- `@url(value, "https")`
- `@timestamp(value, "iso8601"|"rfc3339"|"unix")`
- `@len(value)`
- `@regex(value, pattern)`
- `@empty(value)`
- `@env("NAME")`

## Usage

```gctf
--- ASSERTS ---
@has_header("x-request-id")
@uuid(.user.id, "v4")
@email(.user.email)
@len(.items) > 0
```

## Notes

- External runtime plugin commands are not part of current CLI
- Plugin support is native and loaded by default

## Development

Built-in plugins are implemented as native Rust modules in this repository.

To add a new built-in plugin:

1. Add a plugin module in `src/plugins/`
2. Register it in `src/plugins/mod.rs`
3. Add parser/execution tests for the new behavior
4. Update docs with a usage example

## Related

- [Assertion Reference](../reference/api/assertions.md)
- [Type Validation](../reference/api/type-validation.md)
