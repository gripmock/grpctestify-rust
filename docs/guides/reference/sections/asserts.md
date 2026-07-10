# ASSERTS

Expression-based checks for response or error data.

## When to use

- Prefer for resilient checks that survive minor payload changes
- Use alone or with `RESPONSE with_asserts` / `ERROR with_asserts`

## Minimal example

```gctf
--- ASSERTS ---
.status == "ok"
@len(.items) > 0
@has_header("x-request-id")
```

## Type annotations

When the type of a field is unknown, annotate it with `:type` to allow ordering operators:

```gctf
--- ASSERTS ---
.price:number >= 0
.name:string contains "hello"
```

## Variables from EXTRACT

Use `$name` to reference extracted variables — their type propagates automatically:

```gctf
--- ASSERTS ---
$price >= 0            # type from EXTRACT
$name:string contains "hello"
```

## Rules

- Each line is evaluated as a boolean expression
- Start with high-signal checks first (status, IDs, required fields)

## Related

- [Assertions API Reference](../api/assertions)
- [EXTRACT section](./extract)
