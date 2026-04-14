# Plugin System

Rust version includes built-in assertion plugins available in `ASSERTS`.

Use plugins when basic field comparisons are not enough.

## Available Plugins

- `@header("name")`
- `@has_header("name")`
- `@trailer("name")`
- `@has_trailer("name")`
- `@uuid(value)`
- `@email(value)`
- `@ip(value)`
- `@url(value)`
- `@timestamp(value)`
- `@len(value)`
- `@regex(value, pattern)`
- `@empty(value)`
- `@env("NAME")`
- `@elapsed_ms()`
- `@total_elapsed_ms()`
- `@scope_message_count()`
- `@scope_index()`

## Quick choice

- Metadata checks: `@header`, `@trailer`, `@has_header`, `@has_trailer`
- Format checks: `@uuid`, `@email`, `@url`, `@ip`, `@timestamp`
- Utility checks: `@len`, `@regex`, `@empty`, `@env`
- Timing checks: `@elapsed_ms`, `@total_elapsed_ms`, `@scope_message_count`, `@scope_index`

## Notes

- External runtime plugin commands are not part of current CLI
- Plugin support is native and loaded by default

## For contributors

Built-in plugins live in `src/plugins/` and are registered in `src/plugins/mod.rs`.

## Related

- [Assertion Reference](../reference/api/assertions)
- [ASSERTS section](../reference/sections/asserts)
