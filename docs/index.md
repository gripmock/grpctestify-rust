---
layout: home
hero:
  name: gRPC Testify
  text: Native gRPC test runner
  tagline: CLI for `.gctf` tests (unary + streaming)
  actions:
    - theme: brand
      text: Installation
      link: /guides/getting-started/installation
    - theme: alt
      text: GitHub
      link: https://github.com/gripmock/grpctestify-rust
features:
  - title: Test Format
    details: `.gctf` sections for requests, expected responses, assertions, TLS, and headers.
  - title: Execution
    details: Parallel run, timeout, dry-run, write mode, and coverage.
  - title: Tooling
    details: `check`, `fmt`, `inspect`, `explain`, `reflect`, and `lsp` commands.
---

## Quick Start

```bash
brew tap gripmock/tap
brew install gripmock/tap/grpctestify
grpctestify --version
```

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
hello.HelloService/SayHello

--- REQUEST ---
{ "name": "World" }

--- ASSERTS ---
.message == "Hello, World!"
```

```bash
grpctestify hello.gctf
```
