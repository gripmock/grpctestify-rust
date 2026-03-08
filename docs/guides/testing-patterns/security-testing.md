# Security Testing

Use `REQUEST_HEADERS` and `TLS` sections for secure endpoints.

## Auth headers

```gctf
--- REQUEST_HEADERS ---
authorization: Bearer token123
x-api-key: key123
```

## TLS / mTLS

```gctf
--- TLS ---
ca_cert: ./certs/ca.pem
cert: ./certs/client.pem
key: ./certs/client-key.pem
server_name: api.example.com
insecure: false
```

## Validate metadata

```gctf
--- ASSERTS ---
@has_header("x-request-id")
@trailer("grpc-status") != null
```
