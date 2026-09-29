# ADDRESS

Where the call goes.

- `.gctf` — a gRPC target as `host:port`, or a scheme the transport understands.
- `.httf` — an HTTP origin, `http://host:port` or `https://host`. Without a scheme, `http://` is implied.

## When to use

- Use in the file when the target is fixed
- Omit when the target is supplied by the environment or `GRPCTESTIFY_ADDRESS`

## Minimal example

```gctf
--- ADDRESS ---
localhost:4770
```

```httf
--- ADDRESS ---
https://api.example.com
```

## What wins

In order:

1. The document's own `ADDRESS`
2. The address the chain started with — see below
3. In the playground only: the address of the active environment (`GRPC_ADDRESS` in
   `.grpctestify/.env.<name>`). The CLI (`run`, `call`, `bench`) never reads `.env.<name>` files
4. `GRPCTESTIFY_ADDRESS`
5. The transport's default (`localhost:4770` for gRPC; an HTTP call has no default and needs an address)

## In a chain

A file splits into steps at every `ENDPOINT`. A step without its own `ADDRESS` dials the one the
chain started with, so a chain names its target once:

```httf
--- ADDRESS ---
http://127.0.0.1:8899

--- ENDPOINT ---
GET /v1/users

--- EXTRACT ---
user = .id

--- ENDPOINT ---
GET /v1/users/{{user}}
```

Both steps dial `http://127.0.0.1:8899`. A later step that declares its own `ADDRESS` keeps it, and
the steps after that one inherit that address instead.

## Rules

- One `ADDRESS` per document
- Keep environment-specific values in env vars for CI portability

## Related

- [Test File Format](../api/test-files)
- [HTTP files](../http-files)
