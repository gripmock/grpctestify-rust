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
- **Proto definitions** — go-to-definition on an ENDPOINT's `pkg.Service`/`Method` jumps to its declaration
  in the `.proto` source. Requires `PROTO.files=` + `import_paths=` (local `.proto` compilation); schemas loaded
  via server reflection have no source file to jump to.

## VS Code Setup

The VS Code extension lives in its own repository:
[gripmock/grpctestify-vscode](https://github.com/gripmock/grpctestify-vscode). Follow its README to install
a packaged `.vsix` (or build from source).

Once installed, open any `.gctf` file — the LSP server starts automatically (it spawns `grpctestify lsp`,
so `grpctestify` must be on `PATH`; the repo's README covers the `grpctestify.serverPath` setting). The extension
also ships `.gctf` syntax highlighting and bracket/comment support, on top of every LSP feature listed above.

## Starting Manually

```bash
grpctestify lsp
```

The LSP server listens on stdin/stdout following the LSP protocol (`--stdio` is
the default and only supported transport). Most users should use the VS Code
extension instead.

## Other editors

`grpctestify lsp` is a standard stdio LSP server, so any editor with generic
LSP-client support can drive it — you just need to tell the editor which
command to spawn for `.gctf` files. There's no bundled tree-sitter grammar or
TextMate scope for `.gctf` outside the VS Code extension, so these setups get
diagnostics/completions/hover/etc. but not syntax highlighting.

### Neovim (0.8+)

No plugin required — `vim.lsp.start` wires up an arbitrary server directly:

```lua
vim.filetype.add({ extension = { gctf = "gctf" } })

vim.api.nvim_create_autocmd("FileType", {
  pattern = "gctf",
  callback = function(args)
    vim.lsp.start({
      name = "grpctestify",
      cmd = { "grpctestify", "lsp" },
      root_dir = vim.fs.root(args.buf, { ".git", ".grpctestify" }) or vim.fn.getcwd(),
    })
  end,
})
```

### Helix

Add to `~/.config/helix/languages.toml`:

```toml
[[language]]
name = "gctf"
scope = "source.gctf"
file-types = ["gctf"]
roots = []
language-servers = ["grpctestify-lsp"]

[language-server.grpctestify-lsp]
command = "grpctestify"
args = ["lsp"]
```

### Zed

Zed needs a minimal extension to register a genuinely new language and bind
an LSP adapter to it — a `settings.json`-only setup can only associate a new
file extension with an *existing* language, not invent both the language and
its server from scratch. Until a `grpctestify` Zed extension exists, the
practical options are: treat `.gctf` as plain text/YAML for editing (`"file_types": { "Plain Text": ["gctf"] }`)
and keep using `grpctestify check`/`lsp` diagnostics from the terminal or
another editor, or write a small extension following
[Zed's language extension guide](https://zed.dev/docs/extensions/languages).

## Inlay Hints

When enabled, the LSP shows return types of assertion expressions inline:

```gctf
--- ASSERTS ---
@is_uuid(.id)                 → bool
@len(.items) > 0              → bool
.elapsed_ms < 1000            → bool
```
