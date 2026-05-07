# Command Line Interface

Reference for the Rust CLI.

## Synopsis

```bash
grpctestify [OPTIONS] [TEST_PATHS]... [COMMAND]
```

## Quick workflow

- If no subcommand is provided, `grpctestify` runs tests using the provided paths
- `run` is available explicitly, but optional for normal usage
- Global flags apply to commands (`-v`, `-c`, `--completion`)
- Typical flow: `check` -> `run` -> report flags in CI

## Precedence quick map

- `run` mode runtime keys: `section attributes > OPTIONS > CLI runtime baseline/defaults`
- `bench` mode profile keys: `CLI bench flags > BENCH section > bench defaults`
- Address/TLS/compression also involve env fallbacks; see [OPTIONS](../sections/options) and [BENCH](../sections/bench).

## Naming migration note

- Runtime option/attribute canonical naming is snake_case (`retry_delay`, `no_retry`, `#[retry_delay]`, `#[no_retry]`).

## Commands

- `run [TEST_PATHS]...` - run tests (default command)
- `bench [TEST_PATHS]...` - run load benchmark mode for `.gctf` scenarios
- `check <FILES...>` - validate `.gctf` syntax and semantic rules
- `fmt <FILES...>` - format `.gctf` files
- `inspect <FILE>` - inspect parsed file structure (`text` or `json`)
- `explain <FILE>` - show execution explanation (`text` or `json`)
- `list [PATH]` - list discovered tests for tooling and IDE integration
- `reflect [SYMBOL]` - list reflected services and methods from a target server
- `lsp` - start language server protocol mode

## Global options

- `-v, --verbose` - verbose output
- `-c, --no-color` - disable colorized output
- `--completion <SHELL_TYPE>` - install shell completion (`bash`, `zsh`, `fish`, `elvish`, `powershell`)

## Run options

- `--exclude <PATTERN>` - exclude files/directories by glob (repeatable)
- `--tags <TAGS>` - include only tests containing all provided tags (from `META.tags`)
- `--skip-tags <TAGS>` - exclude tests containing any provided tags (from `META.tags`)
- `-p, --parallel <N|auto>` - parallel workers (`auto` by default)
- `-d, --dry-run` - print execution plan without running requests
- `-s, --sort <TYPE>` - sort discovered test files (default `path`)
- `--log-format <FORMAT>` - file report format (`json`, `junit`, `allure`)
- `--log-output <OUTPUT_FILE>` - output path for file report
- `--stream` - emit streaming JSON events for integration
- `-t, --timeout <SECONDS>` - per-test timeout (default `30`)
- `-r, --retry <COUNT>` - retry count for failed network calls (default `0`)
- `--retry-delay <SECONDS>` - initial retry delay (default `1`)
- `--no-retry` - disable retry mechanisms completely
- `--progress <MODE>` - progress mode (`auto`, `dots`, `bar`, `none`)
- `--no-assert` - skip assertion evaluation and print raw responses
- `--coverage` - generate API coverage report
- `--coverage-format <text|json>` - coverage output format
- `-w, --write` - write actual server responses back to test files (snapshot mode)

Note: if `--log-format` is set without `--log-output`, the run continues and file report generation is skipped with a warning.

## Subcommand options

- `fmt`: `-w, --write` rewrites files in place (without `-w`, checks formatting)
- `check`: `--format <text|json>`
- `inspect`: `--format <text|json>`
- `explain`: `--format <text|json>`
- `list`: `--format <text|json>`, `--with-range`
- `reflect`: `--address <ADDR>`, `--plaintext`
- `lsp`: `--stdio`
- `bench` (selected):
  - stop conditions: `-n, --requests`, `-d, --duration`, `--max-duration`
  - load profile: `--max-rps`, `--load-schedule`, `--load-start`, `--load-step`, `--load-end`, `--load-step-duration`, `--load-max-duration`
  - methodology: `--warmup`, `--ramp-up`, `--duration-stop`, `--skip-first`, `--count-errors-in-latency`, `--latency-percentiles`
  - runtime/transport: `-c, --concurrency`, `--connections`, `--connect-timeout`, `--keepalive`, `--cpus`
  - validation/progress: `--assert-mode`, `--no-assert`, `--sample-rate`, `--progress-interval`
  - metadata/output: `--name`, `--log-format`, `--log-output`

## Bench examples

```bash
# Constant profile for 60 seconds
grpctestify bench tests/ --duration 60s --concurrency 16 --max-rps 200

# Step profile (ghz-style)
grpctestify bench tests/ \
  --duration 40s \
  --load-schedule step \
  --load-start 50 \
  --load-step 10 \
  --load-end 150 \
  --load-step-duration 5s

# Use BENCH section defaults, override progress heartbeat
grpctestify bench tests/ --progress-interval 2s
```

`reflect --plaintext` expects `http://...` or `host:port` addresses. It is rejected for explicit `https://...` addresses.

## Examples

```bash
# Run a single test
grpctestify test.gctf

# Run a directory in parallel
grpctestify tests/ --parallel 4

# Run explicit command form
grpctestify run tests/

# Create JUnit report
grpctestify tests/ --log-format junit --log-output test-results.xml

# Stream JSON events for integrations
grpctestify tests/ --stream

# Use include/exclude filtering
grpctestify tests/ --exclude "tests/legacy/**" --tags smoke --skip-tags flaky

# Validate files
grpctestify check tests/**/*.gctf

# Reflect one method signature
grpctestify reflect user.UserService/GetUser --address localhost:50051

# Format files in-place
grpctestify fmt -w .

# Check formatting (non-zero exit if changes are needed)
grpctestify fmt .
```

## Fmt behavior

- `grpctestify fmt <files...>` works as a formatting check and exits with code `1` if any file needs reformatting.
- `grpctestify fmt -w <files...>` rewrites files in place.
- Safe optimizer rewrites are applied by default.
- For CI, run both `fmt` and `check`.

## See Also

- [Test File Format](./test-files)
- [Installation](../../getting-started/installation)
