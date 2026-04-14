# Installation

Install and set up gRPC Testify (Rust).

Homebrew is usually the fastest on macOS/Linux.

## Quick decision

- macOS/Linux developer machine -> Homebrew
- Rust-first workflow -> Cargo
- CI or pinned binary setup -> GitHub Releases

## Prerequisites

- A reachable gRPC endpoint for validation runs

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

Expected result: command runs without errors and prints version/help.

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

1. [Write Your First Test](first-test)
2. [Learn Basic Concepts](basic-concepts)
3. [Read CLI Reference](../reference/api/command-line)

## Troubleshooting

- If `grpctestify` is not found, ensure install path is in `PATH`
- If connection fails, verify your gRPC server address and availability
- For TLS issues, validate your cert paths in `TLS` section
