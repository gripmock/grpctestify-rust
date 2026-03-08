# Plugin Development

In the Rust version, assertion plugins are implemented as native Rust modules and built into the binary.

## Current State

- Built-in plugins are available out of the box
- Runtime plugin generation commands are not available
- Runtime loading of external plugin files is not supported

## For Contributors

If you want to add a new built-in plugin:

1. Add a plugin module in `src/plugins/`
2. Register it in `src/plugins/mod.rs`
3. Add parser/execution tests for new assertion behavior
4. Update docs with usage examples

## Example Assertion

```gctf
--- ASSERTS ---
@regex(.message, "^ok")
```

## Related

- [Plugin Overview](../)
- [State API](./state-api.md)
