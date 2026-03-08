# Development & CI/CD

Resources for developers and continuous integration with gRPC Testify.

## CI/CD Integration

### GitHub Actions

```yaml
name: gRPC Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install gRPC Testify
        run: |
          brew tap gripmock/tap
          brew install gripmock/tap/grpctestify

      # 1) Formatting gate (check mode, non-zero if reformat is needed)
      - name: Check GCTF formatting
        run: |
          grpctestify fmt .

      # 2) Syntax/semantic validation gate
      - name: Validate GCTF files
        run: |
          grpctestify check .

      # 3) Runtime execution gate
      - name: Run Tests
        run: |
          grpctestify tests/ --log-format junit --log-output results.xml
      - name: Publish Test Results
        uses: dorny/test-reporter@v1
        with:
          name: gRPC Tests
          path: results.xml
          reporter: java-junit
```

### Recommended CI order

1. `grpctestify fmt <paths...>` - style/formatting check (fails if files need changes)
2. `grpctestify check <paths...>` - parse + structural + semantic validation
3. `grpctestify <paths...>` - real execution against gRPC service

This split keeps failure causes explicit (style vs validation vs runtime) and mirrors common tooling workflows (`rustfmt --check` + linters/tests).

### Docker Integration

```dockerfile
FROM rust:1.89-alpine AS builder
RUN cargo install --git https://github.com/gripmock/grpctestify-rust grpctestify

FROM alpine:latest
COPY --from=builder /usr/local/cargo/bin/grpctestify /usr/local/bin/grpctestify
COPY tests/ /tests/
CMD ["grpctestify", "/tests"]
```

### Environment Variables

- `GRPCTESTIFY_ADDRESS` - default server address (`localhost:4770`)
- `GRPCTESTIFY_COMPRESSION` - request compression mode (`none`)

## Contributing

### Development Setup

```bash
# Clone and setup
git clone https://github.com/gripmock/grpctestify-rust.git
cd grpctestify-rust

# Install dependencies
cargo build

# Run tests
cargo test
```

### Code Style

- Follow Rust best practices and clippy suggestions
- Use consistent naming conventions
- Add comments for complex logic
- Include tests for new features

### Release Process

1. Update version numbers
2. Update changelog
3. Create release branch
4. Run full test suite
5. Create GitHub release

## Resources

- [GitHub Repository](https://github.com/gripmock/grpctestify-rust)
- [Issue Tracker](https://github.com/gripmock/grpctestify-rust/issues)
- [API Reference](../guides/reference/)
- [Examples](../guides/examples/)
