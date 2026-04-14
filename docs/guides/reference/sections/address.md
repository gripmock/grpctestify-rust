# ADDRESS

Target gRPC server address in `host:port` format.

## When to use

- Use in the file when test target is fixed
- Omit when target is supplied by `GRPCTESTIFY_ADDRESS`

## Minimal example

```gctf
--- ADDRESS ---
localhost:4770
```

## Rules

- One `ADDRESS` per document
- Keep environment-specific values in env vars for CI portability

## Related

- [Test File Format](../api/test-files)
