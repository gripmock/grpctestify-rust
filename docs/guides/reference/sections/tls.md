# TLS

TLS/mTLS connection settings for secure endpoints.

## When to use

- Server requires TLS or mTLS
- Local tests need custom CA or server name override

## Minimal example

```gctf
--- TLS ---
ca_cert: ./certs/ca.pem
cert: ./certs/client.pem
key: ./certs/client-key.pem
server_name: api.example.com
insecure: false
```

## Supported keys

- `ca_cert` or `ca_file`
- `cert`, `client_cert`, or `cert_file`
- `key`, `client_key`, or `key_file`
- `server_name`
- `insecure`

## Rules

- Use existing certificate files; invalid paths fail at runtime
- Use `insecure: true` only for local/test environments

## Related

- [Troubleshooting](../../troubleshooting)
