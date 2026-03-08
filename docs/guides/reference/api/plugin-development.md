# Plugin Development

The Rust CLI uses built-in assertion plugins implemented in `src/plugins/`.

## Important

- Runtime plugin files are not loaded from disk
- Commands like `--create-plugin`/`--list-plugins` are not available
- Plugin behavior is native and loaded automatically at startup

## Built-in Plugin Functions

Use plugin functions in `ASSERTS`, for example:

```gctf
--- ASSERTS ---
@header("x-request-id") != null
@uuid(.user.id, "v4")
@email(.user.email)
@url(.profile.website, "https")
```

## Contributing New Plugins

To add a new built-in plugin:

1. Implement a plugin module in `src/plugins/`
2. Register it in `src/plugins/mod.rs`
3. Add tests for parser/assertion behavior
4. Document usage in this section

## Related

- [Plugin System](../../plugins/)
- [Type Validation](./type-validation.md)
- [Assertions](./assertions.md)
