# Assertion Patterns

Practical patterns for stable assertions.

## Start simple

```gctf
--- ASSERTS ---
.success == true
.id != null
```

## Validate structure, not full payload

```gctf
--- ASSERTS ---
.user.id | type == "string"
.user.email | test("@")
.roles | length > 0
```

## Metadata checks

```gctf
--- ASSERTS ---
@has_header("x-request-id")
!@has_trailer("grpc-status-details-bin")
```

## Streaming responses

Use multiple `ASSERTS` blocks in response order.

```gctf
--- ASSERTS ---
.stage == "started"

--- ASSERTS ---
.stage == "done"
```
