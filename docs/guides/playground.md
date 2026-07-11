# Playground (`grpctestify play`)

Web UI for interactive gRPC calls. Use it to explore APIs, build requests, and save them as `.gctf` files for CI.

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

```
.grpctestify/
├── settings.json          # project defaults (address, protocol, active env)
├── .env.example           # env template — share with team
├── .env.example.local     # local overrides — gitignored
├── collections/           # .gctf files — commit these
└── history/               # call log (NDJSON)
```

## What it solves

| Before | After |
|--------|-------|
| grpcurl one-liners you lose | Saved `.gctf` files in git |
| Secrets in terminal history | `.env.*.local` gitignored |
| No env separation | `.env.staging`, `.env.prod` with `{{VAR}}` syntax |
| "How did I call that endpoint?" | History panel + NDJSON file |
| Manual JSON construction | Reflect + Auto-fill from proto schema |

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
