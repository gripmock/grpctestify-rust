# State API

The internal state model is used by the CLI engine and built-in plugins.

The State API is not exposed as a public runtime interface.

## What is available now

- Built-in plugins use internal Rust state structures
- Test reports (`json`, `junit`, `allure`) provide execution data for external tooling
- You can consume streamed events with `--stream` for IDE/automation integration

## For extension needs

If you need a stable external extension API, open a feature request:

https://github.com/gripmock/grpctestify-rust/issues
