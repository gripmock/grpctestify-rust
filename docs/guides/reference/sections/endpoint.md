# ENDPOINT

What is called: a gRPC method in a `.gctf`, an HTTP method and path in a
[`.httf`](../http-files).

## When to use

- Required in every test
- Defines which RPC method receives `REQUEST`

## Minimal example

```gctf
--- ENDPOINT ---
user.UserService/GetUser
```

In an HTTP file the same section carries the method and the path:

```httf
--- ENDPOINT ---
POST /v1/users
```

## Rules

- Exactly one `ENDPOINT` is required
- If missing, validation fails
- A chain splits on `ENDPOINT`, in both families
- `package.Service/Method` in a `.gctf`, `<METHOD> <path>` in a `.httf` — each is an error in the
  other family, and `check` says which shape it expected

## Related

- [Test File Format](../api/test-files)
