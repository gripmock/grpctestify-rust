# TLS and mTLS

All TLS is pure-Rust (rustls) — no system OpenSSL, no extra setup.

## Plain TLS (server certificate from a public CA)

If the server uses a certificate your system trusts, an `https://` address
is enough — no `TLS` section needed:

```gctf
--- ADDRESS ---
https://api.example.com:443

--- ENDPOINT ---
user.UserService/GetUser

--- REQUEST ---
{ "id": 1 }

--- ASSERTS ---
.id == 1
```

A bare `host:port` address without a `TLS` section connects in plaintext.

## Custom CA

For internal servers signed by your own CA:

```gctf
--- ADDRESS ---
internal.corp:8443

--- ENDPOINT ---
user.UserService/GetUser

--- TLS ---
ca_cert: ./certs/ca.pem

--- REQUEST ---
{ "id": 1 }

--- ASSERTS ---
.id == 1
```

Relative cert paths resolve against the `.gctf` file's directory, so tests
stay portable inside a repo.

## Mutual TLS (client certificate)

```gctf
--- TLS ---
ca_cert: ./certs/ca.pem
cert: ./certs/client.pem
key: ./certs/client-key.pem
server_name: api.example.com
```

`server_name` overrides SNI/hostname verification — useful when you connect
by IP or through a tunnel while the certificate names the real host.

## Skipping verification (local only)

```gctf
--- TLS ---
insecure: true
```

Connects over TLS but accepts any server certificate. The CLI prints a
security warning. Never use outside local/test environments.

## Environment defaults

Set once, apply to every test that has no explicit `TLS` values:

```bash
export GRPCTESTIFY_TLS_CA_FILE=./certs/ca.pem
export GRPCTESTIFY_TLS_CERT_FILE=./certs/client.pem
export GRPCTESTIFY_TLS_KEY_FILE=./certs/client-key.pem
export GRPCTESTIFY_TLS_SERVER_NAME=api.example.com
```

Explicit `TLS` section keys win over environment defaults.

## CLI commands

`reflect`, `scaffold`, and `health` take the same material as flags:

```bash
grpctestify reflect --address api.example.com:443 --tls-ca ./certs/ca.pem
grpctestify scaffold user.UserService/GetUser --reflect --address api.example.com:443 --tls
grpctestify health --address api.example.com:443 --tls
```

In the `play` UI, the session's TLS lives in the connection chip beside the address — transport,
security and timeout in one place. A file that carries its own `TLS` section wins over it for every
call made from that file, the way it does for a run. Certificate paths there are read as written,
relative to the file that names them: they are paths, not `{{VARIABLES}}`, and the workbench respells
them when the file is moved or saved elsewhere.

## Troubleshooting

- `Failed to read CA certificate` — path is wrong relative to the `.gctf`
  file (or the flag's working directory).
- Certificate name mismatch — set `server_name` to the name in the
  certificate.
- Works with `insecure: true` but not without — the server's chain isn't
  signed by your `ca_cert`; check you exported the full chain.

## Related

- [TLS section reference](../reference/sections/tls)
- [Troubleshooting](../troubleshooting)
