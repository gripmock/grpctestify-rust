# Installation

Install and set up gRPC Testify (Rust).

## Prerequisites

- Docker is optional (for example servers)

## Install

### Homebrew (macOS and Linux)

```bash
brew tap gripmock/tap
brew install gripmock/tap/grpctestify
```

### Cargo

```bash
cargo install grpctestify
```

### Prebuilt binaries (GitHub Releases)

```bash
# Linux/macOS example
curl -LO https://github.com/gripmock/grpctestify-rust/releases/latest/download/grpctestify-linux-amd64.tar.gz
tar -xzf grpctestify-linux-amd64.tar.gz
sudo install -m 0755 grpctestify /usr/local/bin/grpctestify
```

For other targets (macOS arm64/amd64, Linux arm64, Windows amd64/arm64), use release assets:

[GitHub Releases](https://github.com/gripmock/grpctestify-rust/releases/latest)

## Verify Installation

```bash
grpctestify --version
grpctestify --help
```

## Quick Check

```bash
cat > test.gctf << 'EOF'
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
grpc.health.v1.Health/Check

--- REQUEST ---
{}

--- ASSERTS ---
.status == "SERVING"
EOF

grpctestify test.gctf
```

## Next Steps

1. [Write Your First Test](first-test.md)
2. [Learn Basic Concepts](basic-concepts.md)
3. [Explore Examples](../examples/)

## Troubleshooting

- If `grpctestify` is not found, ensure install path is in `PATH`
- If connection fails, verify your gRPC server address and availability
- For TLS issues, validate your cert paths in `TLS` section

## Support

- [Troubleshooting Guide](../troubleshooting)
- [GitHub Issues](https://github.com/gripmock/grpctestify-rust/issues)
