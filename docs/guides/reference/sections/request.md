# REQUEST

Input payload sent to the gRPC method.

## When to use

- Always needed for request-bearing methods
- Multiple `REQUEST` blocks are valid for streaming scenarios

## Minimal example

```gctf
--- REQUEST ---
{
  "user_id": "123"
}
```

## Rules

- Content must be valid JSON
- For client/bidi streaming, order of `REQUEST` blocks matters

## Multiple messages (client/bidi streaming)

For client- or bidi-streaming methods that send several messages, either form works and both send messages in the order written:

- Multiple `REQUEST` blocks, one message each (as above)
- A single `REQUEST` block containing several self-delimiting JSON values, one per line — the same newline-delimited form `RESPONSE` already supports for multi-message expectations:

```gctf
--- REQUEST ---
{ "message": "Hello" }
{ "message": "World" }
{ "message": "Test" }
```

A single JSON value always stays unary — existing single-value `REQUEST` files are unaffected.

## Related

- [Test File Format](../api/test-files)
