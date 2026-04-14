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

## Related

- [Test File Format](../api/test-files)
