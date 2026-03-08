# API Reference

Reference pages for CLI usage and `.gctf` syntax.

## Sections

- [Command Line Interface](./command-line)
- [Test File Format](./test-files)
- [Assertions](./assertions)
- [Type Validation](./type-validation)
- [Report Formats](./report-formats)
- [Plugin Development](./plugin-development)
- [Plugin System](../../plugins/)

## Quick Example

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
service.Method

--- REQUEST ---
{
  "field": "value"
}

--- ASSERTS ---
.field == "value"
```

## See Also

- [Getting Started](../../getting-started/installation)
- [Examples](../../examples/)
