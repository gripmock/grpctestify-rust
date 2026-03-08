# Command Line Interface

Reference for the Rust CLI.

## Synopsis

```bash
grpctestify [OPTIONS] [TEST_PATHS]... [COMMAND]
```

## Main Commands

- `run` - run tests (default)
- `check` - validate `.gctf` syntax/structure
- `fmt` - format `.gctf` files
- `inspect` - inspect AST/workflow for a file
- `explain` - explain execution flow for a file
- `list` - list test files (IDE discovery)
- `reflect` - query server reflection metadata
- `lsp` - start LSP server

## Global Options

- `-v, --verbose` - verbose output
- `-c, --no-color` - disable colors
- `--config` - print resolved config
- `--init-config <FILE>` - create default config file
- `--completion <SHELL>` - install shell completion (`bash`, `zsh`, `fish`, `elvish`, `powershell`)

## Run Options

- `-p, --parallel <N|auto>` - parallel workers (`auto` by default)
- `-d, --dry-run` - show execution plan without running
- `-s, --sort <TYPE>` - sort input files
- `--log-format <FORMAT>` - report format (`json`, `junit`, `allure`)
- `--log-output <FILE>` - report output file
- `--stream` - JSON event stream output
- `-t, --timeout <SECONDS>` - per-test timeout (default `30`)
- `-r, --retry <COUNT>` - retry count (default `0`)
- `--retry-delay <SECONDS>` - initial retry delay (default `1`)
- `--no-retry` - disable retries
- `--progress <MODE>` - progress style (`auto`, `dots`, `bar`, `none`)
- `--no-assert` - skip assertions and print raw responses
- `--coverage` - generate API coverage report
- `--coverage-format <text|json>` - coverage output format
- `-w, --write` - snapshot mode: write actual responses back to files

Note: retry-related flags are currently compatibility options.
Prefer timeout, parallel, and reporting controls for deterministic behavior.

## Examples

```bash
# Run a single test
grpctestify test.gctf

# Run a directory in parallel
grpctestify tests/ --parallel 4

# Create JUnit report
grpctestify tests/ --log-format junit --log-output test-results.xml

# Validate files
grpctestify check tests/**/*.gctf

# Format files in-place
grpctestify fmt -w .

# Check formatting (non-zero exit if changes are needed)
grpctestify fmt .
```

## Fmt Behavior

- `grpctestify fmt <files...>` works as a formatting check and exits with code `1` if any file needs reformatting.
- `grpctestify fmt -w <files...>` rewrites files in place.
- Safe optimizer rewrites are applied by default during formatting.

For CI, run both `fmt` and `check`: `fmt` enforces style, while `check` enforces parse/validation/semantics.

## See Also

- [Test File Format](./test-files)
- [Installation](../../getting-started/installation)
