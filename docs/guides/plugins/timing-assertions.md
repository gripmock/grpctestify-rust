# Timing Assertions

Use timing plugins inside `ASSERTS` to validate response latency behavior.

## Available functions

- `@elapsed_ms()` - elapsed milliseconds for the current assertion scope
- `@total_elapsed_ms()` - cumulative elapsed milliseconds across completed scopes
- `@scope_message_count()` - number of response messages in current scope
- `@scope_index()` - current scope index (1-based)

## Scope rules

- If `RESPONSE` section has **one** expected message, timing scope is that single response.
- If `RESPONSE` section has **multiple** expected messages, timing scope is the whole batch section.
- For standalone `ASSERTS`, timing scope is the current `ASSERTS` response/error event.
- For `ERROR with_asserts=true`, timing scope is the current error event.

## Example: batch scope

```gctf
--- RESPONSE with_asserts=true ---
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

## Example: cumulative over two scopes

```gctf
--- RESPONSE with_asserts=true ---
{
  "status": "NOT_SERVING"
}

--- ASSERTS ---
@scope_index() == 1
@elapsed_ms() >= 10
@total_elapsed_ms() >= 10

--- RESPONSE with_asserts=true ---
{
  "status": "SERVING"
}

--- ASSERTS ---
@scope_index() == 2
@elapsed_ms() >= 0
@total_elapsed_ms() >= 10
```
