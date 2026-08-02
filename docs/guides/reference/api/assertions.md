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
@scope.message_count() == 2
@elapsed_ms() >= 10
@total_elapsed_ms() >= 10
```

- `@elapsed_ms()` - elapsed for current assertion scope.
- `@total_elapsed_ms()` - cumulative elapsed across completed assertion scopes.
- `@scope.message_count()` - number of response messages in current scope.
- `@scope.index()` - current scope index (1-based).

Scope behavior:

- Single message in `RESPONSE` section -> single-message scope.
- Multiple messages in one `RESPONSE` section -> batch scope for the whole section.
- `ASSERTS` following `ERROR with_asserts` use the current error event scope.

## Type helpers

```gctf
--- ASSERTS ---
@is_uuid(.user.id)
@is_email(.user.email)
@is_url(.profile.website)
@is_ip(.client_ip)
@is_timestamp(.created_at)
```

The bare forms (`@uuid`, `@email`, `@url`, `@ip`, `@timestamp`, `@empty`) still
work but are deprecated: `check` reports `SEM_D001` and `fmt --write` rewrites
them to the `is_*` names.

## String helpers

Substring, prefix, and suffix checks — written infix, like the other operators:

```gctf
--- ASSERTS ---
.name contains "ell"
.name startsWith "he"
.name endsWith "lo"
```

## Regex literals

Use `/pattern/flags` for inline regex in assertions:

```gctf
--- ASSERTS ---
.name matches /^hello/
.email matches /^.*@example\.com$/i
```

The `matches` operator treats the RHS as a regex pattern. Slash-delimited
literals are compiled at runtime; invalid patterns produce a clear error.

## Type annotations

Type annotations are optional — `Any` type supports all operators. Use a `:type`
annotation when you want to make the intent explicit:

```gctf
--- ASSERTS ---
.price >= 0                  # Any allows ordering — works without annotation
.price:number >= 0           # explicit annotation (optional, same result)
.name:string contains "hello"
.created_at:timestamp >= "2024-01-01"
```

### Variables from EXTRACT

Variables extracted from responses carry their annotated type:

```gctf
--- EXTRACT ---
total:number = .price

--- ASSERTS ---
$total >= 0          # Any allows ordering — works without annotation
$total:number >= 0   # explicit annotation (optional)
```

Use `$name` to reference an EXTRACT variable inside assertions:

```gctf
--- ASSERTS ---
$total >= 0
$name contains "hello"
```

Inside `REQUEST` / `RESPONSE` / `ERROR` payloads use `"{{var}}"` — the template
engine substitutes the value preserving its JSON type:

```json
{"price": "{{total}}"}        # replaced with 42 (number)
{"name": "{{prefix}}-suffix"} # string interpolation: "val-suffix"
```

### Available types

| Annotation | Meaning |
| ---------- | ------- |
| `:bool` | boolean |
| `:uint` | non-negative integer |
| `:number` | any number |
| `:time`, `:timestamp`, `:duration` | time or duration value |
| `:string`, `:regex` | string |
| `:json` | JSON object or array |
| `:yaml` | YAML document |

`uuid`, `email`, `url`, `ip` are treated as `string`. Runtime validation
catches actual type mismatches — a `:number` annotation on a non-numeric
string value produces a cast error at runtime.

`:number`/`:uint` coerce a numeric-looking JSON *string* into a real number
before comparing. This matters because protobuf's JSON mapping encodes
`int64`/`uint64`/`sint64`/`fixed64`/`sfixed64` fields as strings (to avoid
precision loss) — `.big_id:number > 100` works even though `big_id` arrives
as `"123456789012345"`, not a bare number.

## Full jq pipelines

Any line that isn't a plain `.field` comparison or plugin call runs through a full jq engine
([jaq](https://github.com/01mf02/jaq)) — `reduce`, `foreach`, `map`, object/array construction, and string
interpolation all work today, not just the patterns shown above:

```gctf
--- ASSERTS ---
reduce (.items[]) as $i (0; . + $i.amount) == .total
[.items[] | select(.active)] | length > 0
{name: .user.name, active: .user.active} == {name: "Ada", active: true}
"\(.user.name)-verified" == "Ada-verified"
```

One limitation: an `EXTRACT`'d `$name` variable resolves inside simple comparisons and indexing (`$name`,
`$name[0]`, `$name == 3`), but **not** inside a pipeline complex enough to fall through to the jq engine
(`reduce $name[] as ...`, `$name | map(...)`) — those report a clear "undefined variable" error rather than
silently misbehaving. Build the array/object you need to `reduce`/`map` over from `.` directly instead of
from an extracted variable.

[`examples/assertions/jq-pipelines.gctf`][jq-pipelines-example] is a runnable test proving
`reduce`/`map`/object-construction/string-interpolation/`foreach` all pass through a real gRPC call
(against `grpc.health.v1.Health`, so the field names there are `.status`-shaped rather than
`.items`/`.total` like the illustration above — same operators, different schema).

[jq-pipelines-example]: https://github.com/gripmock/grpctestify-rust/blob/master/examples/assertions/jq-pipelines.gctf

## Notes

- `ASSERTS` can be used alone or together with `RESPONSE with_asserts` / `ERROR with_asserts`
- For unary tests, use one style per test: strict `RESPONSE` or `ASSERTS`
- For a full plugin catalog, see [Plugin System](../../plugins/)
