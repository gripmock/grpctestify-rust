# HTTP files (`.httf`)

`grpctestify` runs three families of test file. A `.gctf` calls a gRPC method; a `.httf` makes an
HTTP request; a [`.apif`](./apif-files.md) may do both in one chain. Everything around the call is
the same in all three: the sections, the assertions, `EXTRACT`, chains, `DATASET`, `OPTIONS`, the
reporters and the workbench.

## What a file looks like

```httf
--- ADDRESS ---
https://api.example.com

--- ENDPOINT ---
POST /v1/users

--- REQUEST_HEADERS ---
authorization: Bearer {{TOKEN}}

--- REQUEST ---
{"name": "Ada"}

--- ASSERTS ---
@status() == 201
.name == "Ada"
```

## The call

`ENDPOINT` carries the method and the path — the same section a `.gctf` uses to say what is called:

```httf
--- ENDPOINT ---
POST /v1/users
```

- Any method is accepted, including one this tool has never heard of (`PROPFIND`, `PURGE`).
- A path is joined to `ADDRESS`; an absolute url is used as written and the address is not consulted.
- An address without a scheme is dialled over `http://`.
- `{{variables}}` are substituted in the path, so a step can call what an earlier step extracted.

A chain splits on `ENDPOINT`, as it does for gRPC, and a step without its own `ADDRESS` keeps the one
the chain started with:

```httf
--- ADDRESS ---
https://api.example.com

--- ENDPOINT ---
POST /v1/users

--- REQUEST ---
{"name": "Ada"}

--- EXTRACT ---
user = .id

--- ENDPOINT ---
GET /v1/users/{{user}}

--- ASSERTS ---
.name == "Ada"
```

## The body

`REQUEST` is sent as written. JSON is the common case; anything else is a body too:

```httf
--- ENDPOINT ---
POST /submit

--- REQUEST ---
name=Ada&age=36
```

The `content-type` is the one `REQUEST_HEADERS` names. When the file names none, it is inferred from
the body: JSON, form-encoded, XML, or `text/plain`. A file with no `REQUEST` section sends no body at
all, which is what most `GET`s and `DELETE`s want.

## What must come back

The status is checked the way a `.gctf` checks one: in `ASSERTS`, beside everything else about the
answer. `RESPONSE` compares the body.

```httf
--- ENDPOINT ---
DELETE /v1/users/7

--- ASSERTS ---
@status() == 204
```

- A status assertion alone is a complete test — a `DELETE` that returns nothing has nothing else to
  compare.
- `RESPONSE` takes the same inline options a `.gctf` one does: `partial`, `tolerance`,
  `unordered_arrays`, `redact`.
- A response body that is not JSON is compared as the text it is.

`ASSERTS` reads the body, `@header("…")` reads the response headers and `@status()` reads the code:

```httf
--- ENDPOINT ---
GET /v1/users/7

--- ASSERTS ---
@status() == 200
@header("content-type") | test("application/json")
.name == "Ada"
```

There is no `ERROR` section here: an HTTP call that returns 404 or 500 is a response that arrived
with that status, so it is a `RESPONSE` for the body and `@status()` for the code. A status written
into the header — `--- RESPONSE 404 ---` — is not read, and `check` says where it belongs.

## What an HTTP file does not have

- `PROTO` — there are no descriptors to load.
- `TLS` — `https://` in the address is the transport security.
- `BENCH` — the load runner is gRPC-shaped today.
- The four RPC shapes: a request and a response, not a stream.

`OPTIONS` keeps `timeout`, `retry`, `retry_delay` and `no_retry`; `protocol` and `compression` belong
to the gRPC family.

## Running one

The same commands, without a flag to say which family:

```bash
grpctestify run api/                 # both families, in one run
grpctestify check api/users.httf
grpctestify fmt --write api/
grpctestify play --dir .             # the workbench opens both
```

One request without a file, the way `call` makes a gRPC one:

```bash
grpctestify call -e 'GET /v1/users' --address https://api.example.com -i
grpctestify call -e 'POST /v1/users' --address https://api.example.com \
  -H 'authorization: Bearer t0ken' -d '{"name": "Ada"}'
```

The exit code follows the status: a `4xx` or `5xx` exits non-zero unless `-S` asks for the body of a
failure.

In the workbench an HTTP file gets a method-and-path control where a `.gctf` gets the method picker,
its status badge reads `200 OK`, and `Copy as curl` writes the same call as a command line. A `curl`
command pasted into the import panel fills the request the same way a `grpcurl` one does.
