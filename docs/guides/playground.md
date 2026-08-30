# Playground (`grpctestify play`)

Web UI for interactive calls. Use it to explore APIs, build requests, and save them as test files for
CI — `.gctf` for gRPC, [`.httf`](reference/http-files) for HTTP, and
[`.apif`](reference/apif-files) for a chain that crosses the two. The connection chip carries the
switch: **what this calls · gRPC | HTTP**.

## Quick Start

```bash
# Current directory
grpctestify play

# Specific project
grpctestify play --dir /path/to/project
```

Opens at `http://localhost:4755`.

## `play --init`

Creates `.grpctestify/` in your project:

```text
.grpctestify/
├── settings.json          # project defaults (address, protocol, active env)
├── .env.example           # env template — share with team
├── .gitignore             # keeps *.local, history/, shares/ and reports/ out of git
├── collections/           # .gctf, .httf and .apif files — commit these
├── history/               # call log (NDJSON), one file per session
└── shares/                # request links made from the workbench, with an expiry
```

`reports/` appears beside them the first time a run is asked for one.

You add your own per-environment files next to these — `.env.staging`, `.env.prod`, etc. — and select
the active one in `settings.json`. A `.env.<name>.local` is yours alone: `init` writes none, the
`.gitignore` covers `*.local`, and the workbench puts a value there rather than in the shared file
when the shared file named the variable and left it empty.

The active environment is what `{{NAME}}` resolves from, for `grpctestify run` in CI and for a run
started in the workbench alike. `.env.<name>.local` is layered over `.env.<name>`, and a `--data` row
or a fixture's `EXTRACT` wins over both — they are the narrower answer.

## What it solves

- **grpcurl one-liners you lose** → Saved `.gctf` files in git
- **Secrets in terminal history** → `.env.*.local` gitignored
- **No env separation** → `.env.staging`, `.env.prod` with `{{VAR}}` syntax
- **"How did I call that endpoint?"** → History panel + NDJSON file
- **Manual JSON construction** → Reflect + Auto-fill from proto schema

## Three verbs, and what each one reads

- **Execute** sends what the editors hold right now, over the file's own connection where it has one.
  It makes a call; it does not check anything.
- **Run** reads the file from disk and runs it the way `grpctestify run` does — `OPTIONS`, retries,
  `ASSERTS`, `EXTRACT`, real streaming. Unsaved edits are not in it, and the workbench says so before
  running.
- **Run over the rail** is the same, over every file the tree is showing, writing the report formats
  the run menu is set to.

## Basic workflow

```bash
cd my-grpc-service
grpctestify play --init          # create .grpctestify/
grpctestify play                 # start UI
# → call APIs, save requests as .gctf
# → .gctf files land in .grpctestify/collections/
git add .grpctestify/
git commit -m "add endpoint tests"
# → CI runs: grpctestify .grpctestify/collections/
```

## Reports

A run writes the formats picked in the run menu into `.grpctestify/reports/<run id>/` — a folder per
run, the last 20 kept — and offers each file to download while it is still there. `run --log-format`
writes the same files from the command line.

## Sharing a request

A share writes the request to `.grpctestify/shares/` on the machine running the workbench and hands
back a link to it. Anyone who can reach that server and has the link can read it, until it expires —
7 days by default, 30 at most. On a workbench bound to a network address (see
[On the network](#on-the-network)) the reader needs its token as well: the link alone opens the page
and nothing in it. Credentials are left out unless they are ticked in the dialog, and the
names of what was left out travel so the other side knows what to supply.

## On the network

By default the workbench binds to loopback and is a local tool: no token, and requests whose `Host`
header is not a loopback name are rejected, so a page on another site cannot reach it through your
own DNS.

`--host 0.0.0.0` (or any other non-loopback interface) puts it on the network, where it hands every
file under the project, every value it can read from `.grpctestify/.env.*` and every call it can make
to whoever reaches the port. In that mode it requires a bearer token on every request:

```bash
GRPCTESTIFY_PLAY_TOKEN=$(openssl rand -hex 16) grpctestify play --host 0.0.0.0
```

Without `GRPCTESTIFY_PLAY_TOKEN` it generates one at startup and prints it as part of the link:

```text
🎨 grpctestify play v1.10.3
   ➜  http://0.0.0.0:4755/?token=16e551d9-19a2-46a7-a612-5e19f55457ce
   bound to 0.0.0.0 — every request needs this token
   set GRPCTESTIFY_PLAY_TOKEN to keep one across restarts
```

Open that link once: the page keeps the token for the browser session, sends it as
`Authorization: Bearer …` on every request, and takes it back out of the address bar so it is not
left in a bookmark or a screenshot. A run's event stream cannot carry a header, so its URL carries
`?token=…` instead; the server accepts either.

Anything without the token gets `401`. The token is the whole of the protection — the connection is
plain HTTP, so put it on a network you trust, or in front of a proxy that terminates TLS.
