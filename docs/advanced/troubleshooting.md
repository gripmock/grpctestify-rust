# Troubleshooting

## Connection problems

### Service unavailable

- Verify server is running and reachable
- Confirm `ADDRESS` value or `GRPCTESTIFY_ADDRESS`
- Check TLS settings if server requires TLS/mTLS

### Timeout errors

Use a higher timeout:

```bash
grpctestify test.gctf --timeout 60
```

## Test file problems

### JSON parse errors

- Validate JSON in `REQUEST` / `RESPONSE` / `ERROR`
- Remove trailing commas
- Ensure section markers are correct

### Missing required sections

- `ENDPOINT` is required
- At least one verification block is required: `RESPONSE`, `ERROR`, or `ASSERTS`

## Assertion problems

### Expression fails

- Start with simple checks (`.status == "ok"`)
- Validate paths exist in actual response
- For metadata checks use built-ins like `@header()` / `@trailer()`

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
```

## Environment variables

- `GRPCTESTIFY_ADDRESS`
- `GRPCTESTIFY_COMPRESSION`
- `GRPCTESTIFY_TLS_CA_FILE`
- `GRPCTESTIFY_TLS_CERT_FILE`
- `GRPCTESTIFY_TLS_KEY_FILE`
- `GRPCTESTIFY_TLS_SERVER_NAME`
