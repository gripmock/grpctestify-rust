# Assertions

Use `ASSERTS` to validate responses.

Each line in `ASSERTS` is evaluated as a boolean expression.

Rule of thumb: use `RESPONSE` for exact payload checks, `ASSERTS` for intent checks.

## Basic examples

```gctf
--- ASSERTS ---
.status == "ok"
.count != null
@len(.items) > 0
.user.email | test("@")
```

## Recommended style

- Start with high-signal checks (`.status`, IDs, required fields)
- Prefer semantic checks over full payload equality
- Use direct boolean plugin calls (`@has_header("x-id")`) instead of `== true`
- Use negation for absence checks (`!@has_trailer("grpc-status-details-bin")`)

## Metadata helpers

```gctf
--- ASSERTS ---
@header("x-request-id") != null
@trailer("x-processing-time") != null
```

## Timing helpers

Timing helpers are available inside `ASSERTS` and are most useful with `RESPONSE with_asserts`:

```gctf
--- RESPONSE with_asserts ---
{
  "status": "NOT_SERVING"
}
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_message_count() == 2
@elapsed_ms() >= 10
@total_elapsed_ms() >= 10
```

- `@elapsed_ms()` - elapsed for current assertion scope.
- `@total_elapsed_ms()` - cumulative elapsed across completed assertion scopes.
- `@scope_message_count()` - number of response messages in current scope.
- `@scope_index()` - current scope index (1-based).

Scope behavior:

- Single message in `RESPONSE` section -> single-message scope.
- Multiple messages in one `RESPONSE` section -> batch scope for the whole section.
- `ASSERTS` following `ERROR with_asserts` use the current error event scope.

## Type helpers

```gctf
--- ASSERTS ---
@uuid(.user.id)
@email(.user.email)
@url(.profile.website)
@ip(.client_ip)
@timestamp(.created_at)
```

## String helpers

Preferred canonical operators:

- `contains`
- `startsWith`
- `endsWith`

## Type annotations

Some operators only work with specific types: `>`, `>=`, `<`, `<=` require numbers; `contains`, `startsWith` require strings. When the type is unknown — typically because there's no running gRPC server to provide protobuf schemas — add a `:type` annotation:

```gctf
--- ASSERTS ---
.price:number >= 0
.name:string contains "hello"
@len(.items):uint > 0
.active:bool == true
.created_at:timestamp >= "2024-01-01"
```

### Variables from EXTRACT

Variables extracted from responses carry their annotated type into assertions:

```gctf
--- EXTRACT ---
total:number = .price

--- ASSERTS ---
$total >= 0          # type :number already known from EXTRACT
$total:number >= 0   # explicit annotation (optional, same result)
```

Use `$name` to reference an EXTRACT variable inside assertions:

```gctf
--- ASSERTS ---
$total:number >= 0
$name:string contains "hello"
```

Inside `REQUEST` / `RESPONSE` / `ERROR` payloads use `"{{var}}"` — the template engine substitutes the value preserving its JSON type:

```json
{"price": "{{total}}"}        # replaced with 42 (number)
{"name": "{{prefix}}-suffix"} # string interpolation: "val-suffix"
```

### Available types

| Annotation | Meaning |
|---|---|
| `:bool` | boolean |
| `:uint` | non-negative integer |
| `:number` | any number |
| `:time`, `:timestamp`, `:duration` | time or duration value |
| `:string` | string |
| `:json` | JSON object or array |
| `:yaml` | YAML document |

`uuid`, `email`, `url`, `ip` are treated as `string`.

## Notes

- `ASSERTS` can be used alone or together with `RESPONSE with_asserts` / `ERROR with_asserts`
- For unary tests, use one style per test: strict `RESPONSE` or `ASSERTS`
- For a full plugin catalog, see [Plugin System](../../plugins/)
