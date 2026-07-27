# Why grpctestify

## The one-line answer

A single self-contained binary that runs declarative gRPC test files — no
protoc, no grpcurl, no jq, no bash, no runtime dependencies.

## vs grpcurl

`grpcurl` is a one-shot query tool: you get a response, then parsing,
asserting, and looping live in shell scripts around it. grpctestify replaces
the whole pipeline: a `.gctf` file declares the request, the expected
response or assertions, retries, TLS, and streaming — and `grpctestify run`
executes hundreds of them in parallel with reports (JSON, JUnit, HTML,
Allure).

Coming from grpcurl? The bridge goes both ways:

```bash
# Turn a grpcurl invocation into a .gctf test (add -e to capture the live response)
grpctestify gen grpcurl -plaintext -d '{"id": 1}' localhost:4770 user.UserService/GetUser

# Turn a .gctf test back into a grpcurl command
grpctestify grpcurl test.gctf
```

## vs ghz

`ghz` is a pure load-testing tool. grpctestify covers functional testing
first, with a built-in benchmark mode (`grpctestify bench`) that reuses the
same `.gctf` files — one format for correctness tests and load profiles,
including regression gates against a saved baseline.

## vs the bash grpctestify

This is the native rewrite of the original bash implementation. The bash
version shells out to grpcurl and jq per call; this one embeds a pure-Rust
gRPC client (tonic), proto compiler (protox, no protoc binary), TLS
(rustls, no OpenSSL), and jq engine (jaq). Result: one binary, identical
behavior across platforms, parallel execution, and features the bash
version can't reach — LSP, web playground, Allure reports, coverage,
benchmarks, plugins.

## What you get in the box

- `run` — parallel test execution with retries, coverage, and report formats
- `play` — local web playground for exploring services and editing tests
- `bench` — load testing with data sources and regression gates
- `scaffold` / `gen grpcurl` — generate tests from reflection or grpcurl
- `check` / `fmt` / `lsp` — validation, formatting, editor support
- Custom assertions and reporters as drop-in Rhai scripts

## When not to use it

- You need a GUI-first desktop client for ad-hoc poking — use the built-in
  `play` UI, or a dedicated tool like grpcui.
- You need Postman-style collections shared with non-CLI users.
- Your protocol isn't gRPC/gRPC-Web/Connect.

## Related

- [Installation](getting-started/installation)
- [First Test](getting-started/first-test)
