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
| Variable | Description | Default |
|----------|-------------|---------|
| `GRPCTESTIFY_ADDRESS` | Default server address | `localhost:4770` |
| `GRPCTESTIFY_COMPRESSION` | Request compression mode | `none` |

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
