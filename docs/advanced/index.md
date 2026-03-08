# Advanced Usage

This section covers advanced but supported capabilities of the Rust CLI.

## Useful Commands

```bash
# Explain execution flow for a file
grpctestify explain test.gctf

# Inspect parsed structure
grpctestify inspect test.gctf --format json

# Validate syntax for multiple files
grpctestify check tests/**/*.gctf

# Format files
grpctestify fmt -w tests/**/*.gctf

# Query server reflection
grpctestify reflect package.Service/Method
```

## Reporting

```bash
# JSON
grpctestify tests/ --log-format json --log-output results.json

# JUnit
grpctestify tests/ --log-format junit --log-output results.xml

# Allure
grpctestify tests/ --log-format allure --log-output allure-results
```

## Coverage

```bash
grpctestify tests/ --coverage
grpctestify tests/ --coverage --coverage-format json
```

## Environment Variables

- `GRPCTESTIFY_ADDRESS`
- `GRPCTESTIFY_COMPRESSION` (`none` or `gzip`)
- `GRPCTESTIFY_TLS_CA_FILE`
- `GRPCTESTIFY_TLS_CERT_FILE`
- `GRPCTESTIFY_TLS_KEY_FILE`
- `GRPCTESTIFY_TLS_SERVER_NAME`
