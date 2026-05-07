# Test File Format

Specification of `.gctf` files for the Rust CLI.

Think of a `.gctf` file as: target + input + expected outcome.

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

- Core: `META`, `ADDRESS`, `ENDPOINT`, `REQUEST`, `RESPONSE`, `ERROR`, `ASSERTS`
- Supporting: `EXTRACT`, `REQUEST_HEADERS`, `TLS`, `PROTO`, `OPTIONS`, `BENCH`

For section details, use [Section Reference](../sections/).

## Execution order

- Preamble sections are read first: `ADDRESS`, `TLS`, `PROTO`, `OPTIONS`, `REQUEST_HEADERS`
- Request/validation flow is processed in-order for each RPC interaction
- Multiple `REQUEST`/`RESPONSE`/`ASSERTS` blocks are allowed depending on RPC pattern

## Validation Rules

- `ENDPOINT` is required
- At least one of `RESPONSE`, `ERROR`, or `ASSERTS` is required
- `RESPONSE` and `ERROR` cannot be used together in one file
- `ADDRESS` may be omitted if `GRPCTESTIFY_ADDRESS` is set
- `META` is optional, but only one is allowed and it must be the first section
- `BENCH` is optional, but only one is allowed and it should be first or immediately after `META`

## Attributes

Per-section modifiers using `#[name(value)]` syntax:

```gctf
#[timeout(10)]
#[retry(2)]
--- REQUEST ---
{
  "query": "slow search"
}
```

See [Attributes](../sections/attributes) for full reference.

## RESPONSE Inline Options

Inline options use section-header flags and `key=value` pairs:

```gctf
--- RESPONSE with_asserts partial tolerance=0.1 unordered_arrays ---
{
  "status": "ok"
}
```

Supported options for `RESPONSE`:

- `with_asserts` or `with_asserts=true|false`
- `partial` or `partial=true|false`
- `tolerance=<number>`
- `redact=["field1","field2"]`
- `unordered_arrays` or `unordered_arrays=true|false`

`ERROR` supports:

- `with_asserts` or `with_asserts=true|false`
- `partial` or `partial=true|false`

Default `ERROR` matching is strict for top-level fields (`code`, `message`, `details`).
If `details` is not returned by the server, it can be omitted from expected `ERROR`.

## Quick links by section

- [META](../sections/meta)
- [ADDRESS](../sections/address)
- [ENDPOINT](../sections/endpoint)
- [REQUEST](../sections/request)
- [RESPONSE](../sections/response)
- [ERROR](../sections/error)
- [ASSERTS](../sections/asserts)
- [EXTRACT](../sections/extract)
- [REQUEST_HEADERS](../sections/request-headers)
- [TLS](../sections/tls)
- [PROTO](../sections/proto)
- [OPTIONS](../sections/options)
- [BENCH](../sections/bench)
- [Attributes](../sections/attributes)

Related: [Assertions](./assertions), [Plugin System](../../plugins/).
