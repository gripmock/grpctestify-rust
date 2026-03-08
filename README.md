# gRPC Testify (Rust)

[![Release](https://img.shields.io/github/v/release/gripmock/grpctestify-rust?logo=github)](https://github.com/gripmock/grpctestify-rust/releases/latest)
[![Documentation](https://img.shields.io/badge/Docs-VitePress-646CFF?logo=vitepress)](https://gripmock.github.io/grpctestify-rust/)
[![VS Code Extension](https://img.shields.io/badge/VS_Code-Marketplace-blue?logo=visualstudiocode)](https://marketplace.visualstudio.com/items?itemName=gripmock.grpctestify)

Native CLI for gRPC testing with `.gctf` files.

## Documentation

- [Docs](https://gripmock.github.io/grpctestify-rust/)
- [Generator](https://gripmock.github.io/grpctestify-rust/generator)
- [Repository](https://github.com/gripmock/grpctestify-rust)

## Key Features

- Unary, client streaming, server streaming, and bidirectional streaming tests
- Assertions with built-in operators and plugin functions (`@header`, `@trailer`, `@uuid`, `@email`, etc.)
- Parallel execution, timeouts, coverage, snapshot mode (`--write`)
- Output formats: `console`, `json`, `junit`, `allure`
- Extra tools for developer workflows: `check`, `fmt`, `inspect`, `explain`, `reflect`, `lsp`

## Requirements

- No external runtime dependencies for CLI execution
- Docker is optional (for integration examples)

## Installation

### Homebrew (macOS and Linux)

```bash
brew tap gripmock/tap
brew install gripmock/tap/grpctestify
```

### Cargo

```bash
cargo install --git https://github.com/gripmock/grpctestify-rust grpctestify
```

### Prebuilt binaries (GitHub Releases)

- Download from [GitHub Releases](https://github.com/gripmock/grpctestify-rust/releases/latest)
- Available for macOS, Linux, and Windows (amd64/arm64)

Verify installation:

```bash
grpctestify --version
```

## Quick Start

1. Create `hello.gctf`:

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
hello.HelloService/SayHello

--- REQUEST ---
{
  "name": "World"
}

--- ASSERTS ---
.message == "Hello, World!"
```

1. Run test:

```bash
grpctestify hello.gctf
```

## Common Commands

```bash
# Run tests
grpctestify tests/

# Parallel run
grpctestify tests/ --parallel 4

# JSON report
grpctestify tests/ --log-format json --log-output results.json

# JUnit report
grpctestify tests/ --log-format junit --log-output junit.xml

# Validate syntax
grpctestify check tests/**/*.gctf

# Format files
grpctestify fmt -w tests/**/*.gctf
```

## Contributing

Issues and PRs are welcome: [GitHub Issues](https://github.com/gripmock/grpctestify-rust/issues)

## License

[MIT](LICENSE)
