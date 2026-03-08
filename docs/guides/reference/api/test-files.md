# Test File Format

Specification of `.gctf` files for the Rust CLI.

## Minimal Example

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
package.Service/Method

--- REQUEST ---
{
  "id": 1
}

--- ASSERTS ---
.id == 1
```

## Supported Sections

- `ADDRESS` - server address (`host:port`)
- `ENDPOINT` - gRPC method (`package.Service/Method`)
- `REQUEST` - JSON payload (multiple allowed)
- `RESPONSE` - expected JSON (multiple allowed)
- `ERROR` - expected error JSON/string
- `ASSERTS` - assertion expressions (multiple allowed)
- `EXTRACT` - variable extraction rules (multiple allowed)
- `REQUEST_HEADERS` (or `HEADERS`) - request metadata
- `TLS` - TLS/mTLS config
- `PROTO` - descriptor/reflection configuration
- `OPTIONS` - parsed and validated, but currently not applied at runtime

## Validation Rules

- `ENDPOINT` is required
- At least one of `RESPONSE`, `ERROR`, or `ASSERTS` is required
- `RESPONSE` and `ERROR` cannot be used together in one file
- `ADDRESS` may be omitted if `GRPCTESTIFY_ADDRESS` is set

## RESPONSE Inline Options

Inline options use `key=value` in the section header:

```gctf
--- RESPONSE with_asserts=true partial=true tolerance=0.1 unordered_arrays=true ---
{
  "status": "ok"
}
```

Supported options for `RESPONSE`:

- `with_asserts=true|false`
- `partial=true|false`
- `tolerance=<number>`
- `redact=["field1","field2"]`
- `unordered_arrays=true|false`

`ERROR` supports only `with_asserts=true|false`.

## TLS Section

```gctf
--- TLS ---
ca_cert: ./certs/ca.pem
cert: ./certs/client.pem
key: ./certs/client-key.pem
server_name: api.example.com
insecure: false
```

Supported keys include `ca_cert`/`ca_file`, `cert`/`client_cert`/`cert_file`, `key`/`client_key`/`key_file`, `server_name`, `insecure`.

## PROTO Section

```gctf
--- PROTO ---
descriptor: ./descriptors/api.desc
```

Notes:

- Native mode supports `descriptor=<path>` and server reflection
- `PROTO files=...` is currently not supported in native mode

## Assertion Plugins

Examples of built-in plugins:

```gctf
--- ASSERTS ---
@header("x-request-id") != null
@uuid(.user.id, "v4")
@email(.user.email)
```

See also: [Assertions](./assertions.md), [Type Validation](./type-validation.md).
