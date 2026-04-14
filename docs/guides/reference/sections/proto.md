# PROTO

Descriptor/reflection configuration for method and type resolution.

## When to use

- Reflection is unavailable and you provide descriptors manually
- You need deterministic schema source in CI

## Minimal example

```gctf
--- PROTO ---
descriptor: ./descriptors/api.desc
```

## Rules

- Native mode supports `descriptor: <path>` and server reflection
- `PROTO files=...` is not supported in native mode

## Related

- [Command Line](../api/command-line)
