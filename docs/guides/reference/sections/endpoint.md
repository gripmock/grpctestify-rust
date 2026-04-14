# ENDPOINT

gRPC method in `package.Service/Method` format.

## When to use

- Required in every test
- Defines which RPC method receives `REQUEST`

## Minimal example

```gctf
--- ENDPOINT ---
user.UserService/GetUser
```

## Rules

- Exactly one `ENDPOINT` is required
- If missing, validation fails

## Related

- [Test File Format](../api/test-files)
