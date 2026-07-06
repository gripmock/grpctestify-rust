# Troubleshooting

Quick checklist for unexpected failures.

## Fast triage

1. Run `grpctestify check <file>` to validate syntax/sections.
2. Run the same file with `--verbose`.
3. Reduce concurrency (`--parallel 1`) if failures are flaky.
4. Check endpoint address/TLS settings.

## Connection problems

### Service unavailable

- Verify server is running
- Confirm `ADDRESS` value or `GRPCTESTIFY_ADDRESS`
- Check TLS settings if server requires TLS/mTLS

### Timeout errors

Use a higher timeout:

```bash
grpctestify test.gctf --timeout 60
```

For unstable endpoints, run with reduced parallelism to isolate load-related failures:

```bash
grpctestify tests/ --parallel 1 --verbose
```

If behavior is unexpected because of merged runtime settings, inspect effective values:

```bash
grpctestify inspect test.gctf --format json
grpctestify explain test.gctf
```

Runtime precedence quick map:

- `run`: section attributes > `OPTIONS` > CLI runtime baseline/defaults
- `bench`: CLI bench flags > `BENCH` section > bench defaults

If behavior differs between `run` and `bench`, verify you are reading the correct precedence model:

- `run`: `section attributes > OPTIONS > CLI runtime baseline/defaults`
- `bench`: `CLI bench flags > BENCH section > bench defaults`

## Test file problems

### JSON parse errors

- Validate JSON in `REQUEST` / `RESPONSE` / `ERROR`
- Remove trailing commas
- Ensure section markers are valid

### Missing required sections

- `ENDPOINT` is required
- At least one verification block is required: `RESPONSE`, `ERROR`, or `ASSERTS`
- `RESPONSE` and `ERROR` cannot appear in the same file
- `META` (if present) must be the first section and appear only once

### Unexpected section behavior

- Use `REQUEST_HEADERS` instead of legacy `HEADERS`
- Keep section markers exact: `--- SECTION_NAME ---`
- For inline options, use `key=value` when needed, or short boolean flags like `with_asserts`

## Assertion problems

### Expression fails

- Start with simple checks (`.status == "ok"`)
- Validate paths exist in actual response
- For metadata checks use `@header()` / `@trailer()`

### Type/format checks fail

- Verify values match expected format for `@timestamp`, `@url`, `@ip`, `@email`, `@uuid`
- Check raw response types with `inspect` output before writing strict assertions

## Debugging commands

```bash
# Verbose run
grpctestify test.gctf --verbose

# Execution preview
grpctestify test.gctf --dry-run --verbose

# Syntax check
grpctestify check test.gctf

# Inspect parsed structure
grpctestify inspect test.gctf --format json

# Explain execution plan and assertion scopes
grpctestify explain test.gctf
```

## Environment variables

- `GRPCTESTIFY_ADDRESS`
- `GRPCTESTIFY_COMPRESSION`
- `GRPCTESTIFY_TLS_CA_FILE`
- `GRPCTESTIFY_TLS_CERT_FILE`
- `GRPCTESTIFY_TLS_KEY_FILE`
- `GRPCTESTIFY_TLS_SERVER_NAME`

## FAQ

### How do I run a quick smoke test?

```bash
grpctestify call myservice.test.gctf
```

### How do I see what a test will do without running it?

```bash
grpctestify explain test.gctf
```

### What is a `.gctf` file?

A gRPC Test File. It defines endpoint, request, expected response, and assertions in a plain text format. See [Test Files](reference/api/test-files).

### Why does `grpctestify check` fail on my file?

Run `grpctestify check --verbose` for detailed diagnostics. Common issues: missing required sections, JSON syntax errors, or incorrect assertion expressions.

### Can I test streaming endpoints?

Yes. gRPC Testify supports unary, server streaming, client streaming, and bidirectional streaming. Use the inline options on `ENDPOINT` (e.g., `--- ENDPOINT --- with_stream`).

### How do benchmark sources work?

Define one or more `sources` in the `BENCH` section. Each source is a CSV/TSV/NDJSON file with an optional `indexed_by` column. During bench execution, rows from the primary source drive gRPC requests via template variables. See [Data Sources](bench-sources).

## Related

- [OPTIONS](./reference/sections/options)
- [ATTRIBUTES](./reference/sections/attributes)
- [BENCH](./reference/sections/bench)
