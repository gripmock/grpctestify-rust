# EXTRACT

Extract values from response data for reuse in assertions and downstream requests.

## When to use

- Reuse IDs/tokens returned by service
- Keep assertions concise when values are nested
- Chain values across requests in multi-document tests

## Minimal example

```gctf
--- EXTRACT ---
user_id = .user.id
user_role = .user.role
```

## Type annotations

Add `:type` to the variable name to propagate the type to assertions:

```gctf
--- EXTRACT ---
price:number = .price
name:string = .user.name
created:time = .created_at
```

The type is then known in `ASSERTS` — no need to annotate again:

```gctf
--- ASSERTS ---
$price >= 0
$name contains "hello"
```

## Rules

- Multiple extraction rules are allowed
- Keep rule names short and descriptive
- Use `$name` to reference extracted variables in `ASSERTS`
- Use `"{{name}}"` for template substitution in JSON payloads

## Related

- [ASSERTS section](./asserts)
- [Assertions API](../api/assertions)
- [Test File Format](../api/test-files)
