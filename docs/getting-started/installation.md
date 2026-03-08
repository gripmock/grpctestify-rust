# Installation

Install gRPC Testify (Rust) using the method that fits your environment.

## Requirements

- Docker is optional (for example/integration scenarios)

## Install Options

### Homebrew (macOS and Linux)

```bash
brew tap gripmock/tap
brew install gripmock/tap/grpctestify
```

### Cargo

```bash
cargo install --git https://github.com/gripmock/grpctestify-rust grpctestify
```

### Prebuilt binaries

Download from:

https://github.com/gripmock/grpctestify-rust/releases/latest

Available targets include macOS/Linux/Windows (amd64/arm64).

## Verify

```bash
grpctestify --version
grpctestify --help
```

## Next Step

Continue with [Your First Test](./index.md).
