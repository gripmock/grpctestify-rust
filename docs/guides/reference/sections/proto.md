# PROTO

Descriptor/reflection configuration for method and type resolution.

By default the schema comes from the server's own reflection. Add a `PROTO`
section only when you want to resolve methods and types from local files instead
— when reflection is disabled on the server, or you need a schema source that's
fixed and reproducible in CI.

## When to use

- The server has reflection turned off
- You want a deterministic schema source in CI
- You want go-to-definition in the editor (needs `files` + `import_paths`)

## Keys

- `descriptor: <path>` — a pre-compiled descriptor set (`.desc`/`.pb`)
- `files: <a.proto, b.proto>` — `.proto` sources compiled on the fly
- `import_paths: <dir, dir>` — roots used to resolve imports for `files`

## Examples

```gctf
--- PROTO ---
descriptor: ./descriptors/api.desc
```

```gctf
--- PROTO ---
files: greeter.proto
import_paths: ./proto
```

## Rules

- Without a `PROTO` section, the schema is loaded via server reflection
- Use `descriptor:` or `files:` — not both in the same section
- A key set twice is a parse error (not last-wins)

## Related

- [Command Line](../api/command-line)
