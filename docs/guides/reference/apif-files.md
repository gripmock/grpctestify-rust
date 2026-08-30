# Mixed files (`.apif`)

A `.gctf` calls gRPC and a `.httf` makes HTTP requests. A `.apif` may do both, one step at a time, in
one chain — which is what a flow that crosses services usually needs: take a token from the auth
service over gRPC, read the order from the HTTP gateway with it, post the result to whichever service
takes it.

Everything else is the same file format: the sections, the assertions, `EXTRACT`, `DATASET`,
`OPTIONS`, retries, fixtures, the reporters and the workbench.

## What a file looks like

```apif
--- ADDRESS ---
127.0.0.1:4770

--- ENDPOINT ---
auth.v1.Auth/Login

--- REQUEST ---
{"user": "ada"}

--- ASSERTS ---
.ok == true

--- EXTRACT ---
token = .token

--- ADDRESS ---
https://gateway.example.test

--- ENDPOINT ---
GET /v1/orders

--- REQUEST_HEADERS ---
authorization: Bearer {{token}}

--- ASSERTS ---
@status() == 200
```

## The transport belongs to the step

Each step's own `ENDPOINT` says how it is dialled:

| `ENDPOINT` | transport |
| --- | --- |
| `package.Service/Method` | gRPC |
| `<METHOD> <path>` — `GET /v1/orders`, `POST /v1/users` | HTTP |

The two shapes cannot be confused: a gRPC method never carries a space, and an HTTP endpoint always
does. A line that is neither is reported by `check`, naming both shapes it could have had.

A step's sections are read against that step's transport: `PROTO`, `TLS` and `ERROR` belong to a gRPC
step, `REQUEST_HEADERS` to an HTTP one. An HTTP step's status is a check like any other —
`@status() == 201` in `ASSERTS` — and a `RESPONSE` section compares the body.

## An address belongs to a transport

A step dials the last `ADDRESS` declared **for its own transport** — not the one the chain started
with, which would hand a gRPC `host:port` to an HTTP step. Declare one for each transport once; the
steps after it inherit it.

## Steps that go out together

By default the steps of a chain run in order. `parallel` on an `ENDPOINT` marks a step as one of a
group: a run of consecutive marked steps goes out together, and the chain waits for all of them
before the next step starts.

```apif
--- ENDPOINT parallel ---
GET /v1/orders

--- ASSERTS ---
@status() == 200

--- ENDPOINT parallel ---
catalog.v1.Catalog/List

--- REQUEST ---
{}

--- ASSERTS ---
.items | length > 0
```

- Every step of a group is reported with its own verdict, whatever the others did — the calls were
  already out. The chain stops after the group, not inside it.
- What the group costs the file is its slowest step, not the sum.
- A step cannot read what another step of its own group binds: they go out together, so nothing they
  bind is there for each other. `check` reports it rather than leaving a race in the file.
- `parallel` works in any family — a `.httf` fanning out to four endpoints wants it for the same
  reason.

## What runs it

`run`, `check`, `fmt`, `graph` and `play` take `.apif` files without a flag, and a directory may hold
all three families. `bench` does not: the load runner dials gRPC, and a mixed file holds steps it
cannot send.
