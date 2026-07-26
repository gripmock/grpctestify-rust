# Plugin System

Built-in assertion plugins available in `ASSERTS`. For writing your own — drop a `.rhai` file into `~/.grpctestify/plugins` or `./.grpctestify/plugins`, no flag needed — see:

- [Custom assertion plugins](custom-scripts) — `.rhai` scripts, one file can define many, naming/arity/`private fn`/sandboxing
- [Installing from a git host](installing) — `grpctestify plugins install github.com/owner/repo`, versioning, lockfile
- [Reporter plugins](reporters) — `on_test_start`/`on_test_end`/`on_suite_end` hooks
- [Script stdlib](stdlib) — the native helpers (`log_*`, `is_uuid`, `regex_match`, ...) every script gets for free

## Validation plugins

| Plugin | Checks | Returns |
| --- | --- | --- |
| `@is_uuid(value)` | valid UUID format | bool |
| `@is_email(value)` | valid email address | bool |
| `@is_ip(value)` | valid IP address (v4/v6) | bool |
| `@is_url(value)` | valid URL | bool |
| `@is_timestamp(value)` | valid timestamp | bool |
| `@is_base64(value)` | valid base64 string | bool |
| `@is_json(value)` | valid JSON string | bool |

## State plugins

| Plugin | Checks | Returns |
| --- | --- | --- |
| `@is_empty(value)` | value is null/empty string/empty array/empty object | bool |
| `@has_value(value)` | value is non-null and non-empty | bool |

## Metadata plugins

| Plugin | Returns |
| --- | --- |
| `@header("name")` | header value (string or null) |
| `@has_header("name")` | bool |
| `@trailer("name")` | trailer value (string or null) |
| `@has_trailer("name")` | bool |

## Utility plugins

| Plugin | Returns |
| --- | --- |
| `@len(value)` | non-negative integer |
| `@regex(value, pattern)` | bool |
| `@env("NAME")` | environment variable value (string or null) |

## Timing plugins

| Plugin | Returns |
| --- | --- |
| `@elapsed_ms()` | non-negative integer |
| `@total_elapsed_ms()` | non-negative integer |
| `@scope.message_count()` | non-negative integer |
| `@scope.index()` | non-negative integer |

## Type methods (`@type.method`)

Extract parts from typed values:

| Method | Returns | Example |
| --- | --- | --- |
| `@url.scheme(url)` | `:string` | `"https"` |
| `@url.host(url)` | `:string` | `"example.com"` |
| `@url.port(url)` | `:string` | `"443"` |
| `@url.path(url)` | `:string` | `"/api/v1"` |
| `@url.query(url)` | `:string` | `"?page=1"` |
| `@url.fragment(url)` | `:string` | `"section"` |
| `@email.local_part(email)` | `:string` | `"user"` |
| `@email.domain(email)` | `:string` | `"example.com"` |
| `@ip.version(ip)` | `:uint` | `4` or `6` |
| `@uuid.version(uuid)` | `:uint` | `4` |
| `@json.key(json, "key")` | `:any` | extracted value |

## Quick choice

- Metadata checks: `@header`, `@trailer`, `@has_header`, `@has_trailer`
- Format checks: `@is_uuid`, `@is_email`, `@is_url`, `@is_ip`, `@is_timestamp`, `@is_base64`, `@is_json`
- State checks: `@is_empty`, `@has_value`
- Type methods: `@url.*`, `@email.*`, `@json.*`
- Utility checks: `@len`, `@regex`, `@env`
- Timing checks: `@elapsed_ms`, `@total_elapsed_ms`, `@scope.message_count`, `@scope.index`

## Related

- [Custom assertion plugins](custom-scripts)
- [Installing from a git host](installing)
- [Reporter plugins](reporters)
- [Script stdlib](stdlib)
- [Assertion Reference](../reference/api/assertions)
- [ASSERTS section](../reference/sections/asserts)
