# EXTRACT

Extract values from response data for reuse in later assertions.

## When to use

- Reuse IDs/tokens returned by service
- Keep assertions concise when values are nested

## Minimal example

```gctf
--- EXTRACT ---
user_id = .user.id
user_role = .user.role
```

## Rules

- Multiple extraction rules are allowed
- Keep rule names short and descriptive

## Related

- [ASSERTS section](./asserts)
- [Test File Format](../api/test-files)
