# Streaming

grpctestify supports all four gRPC shapes. The mode is taken from the
method's proto definition (via reflection or your `PROTO` config) — the test
file only describes the messages, no special flags needed.

## Server streaming

One request, several responses. Write one `RESPONSE` section per expected
message, in order:

```gctf
--- ENDPOINT ---
chat.ChatService/ReceiveMessages

--- REQUEST ---
{ "user": "Alice" }

--- RESPONSE ---
{ "from": "Server", "text": "Welcome!" }

--- RESPONSE ---
{ "from": "Server", "text": "Message 1" }

--- RESPONSE ---
{ "from": "Server", "text": "Message 2" }
```

The test fails if the server sends a different number of messages or any
message doesn't match. To assert instead of matching exact payloads, replace
the `RESPONSE` sections with a single `ASSERTS` block — it evaluates against
each received message.

## Client streaming

Several requests, one response. Either separate `REQUEST` sections:

```gctf
--- ENDPOINT ---
chat.ChatService/SendMessages

--- REQUEST ---
{ "from": "Alice", "text": "Message 1" }

--- REQUEST ---
{ "from": "Alice", "text": "Message 2" }

--- RESPONSE ---
{ "count": 2, "status": "OK" }
```

or the compact JSON Lines form — one message per line inside a single
`REQUEST`:

```gctf
--- REQUEST ---
{ "from": "Alice", "text": "Message 1" }
{ "from": "Alice", "text": "Message 2" }
{ "from": "Alice", "text": "Message 3" }
```

## Bidirectional streaming

Combine both: multiple `REQUEST` sections and multiple `RESPONSE` sections.
Requests are sent in order; responses are matched in the order received.

## Asserting on streams

- A standalone `ASSERTS` after the requests evaluates each incoming message.
- `@trailer("key")` / `@has_trailer("key")` work after the stream ends — an
  `ASSERTS` placed to run at end-of-stream can check trailers and status.
- Timeouts apply per read: `timeout=5` as a section attribute bounds the
  wait for each message.

## Trying it interactively

`grpctestify play` shows the real method shape (Unary / Client streaming /
Server streaming / Bidirectional) next to the request editor once reflection
has run, and "Add message" builds multi-message requests.

## Related

- [REQUEST](../reference/sections/request)
- [RESPONSE](../reference/sections/response)
- [ASSERTS](../reference/sections/asserts)
