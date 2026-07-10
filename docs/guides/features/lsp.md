# LSP Support

gRPC Testify includes a built-in Language Server Protocol (LSP) server that provides IDE features for `.gctf` files.

## Features

- **Syntax validation** — real-time error diagnostics as you type
- **Completions** — section names, assertion operators, plugin calls
- **Inlay hints** — inline type information for assertion expressions
- **Document symbols** — outline view of sections, assertions, and extractions
- **Folding ranges** — collapse/expand sections
- **Semantic tokens** — syntax highlighting tokens for rich editor support
- **Variable definitions** — go-to-definition for extracted variables

## VS Code Setup

1. Install the [gRPC Testify VS Code extension](https://marketplace.visualstudio.com/items?itemName=gripmock.grpc-testify)
2. Open any `.gctf` file
3. The LSP server starts automatically

## Starting Manually

```bash
grpctestify lsp
```

The LSP server listens on stdin/stdout following the LSP protocol. Most users should use the VS Code extension instead.

## Inlay Hints

When enabled, the LSP shows return types of assertion expressions inline:

```gctf
--- ASSERTS ---
@uuid(.id)                    → bool
@len(.items) > 0              → bool
.elapsed_ms < 1000            → bool
```
