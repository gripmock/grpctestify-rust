# Output Forms

`run`'s live console output and its file-based reports (`--log-format`) are
independent — you can watch dots on the terminal while writing a JUnit XML
file at the same time. This page covers the live/console/streaming forms;
see [Report Formats](./report-formats) for the file-based ones.

## Console progress (`--progress`)

```bash
grpctestify tests/ --progress dots      # one glyph (✓/✗/○) per test, default in a non-verbose run
grpctestify tests/ --progress verbose   # per-test detail, grouped by file, assertions shown
grpctestify tests/ --progress none      # silent — only the final summary
grpctestify tests/ --progress auto      # dots normally, verbose if -v/--verbose is set (default)
```

`bar` is accepted as an alias for `dots`. `-v`/`--verbose` alone (without an
explicit `--progress`) switches `auto` to verbose.

- **Dots**: a live `✓`/`✗`/`○` per test as it finishes, then the summary.
- **Verbose**: file = suite, assertions grouped by endpoint (`↳`), expected/
  actual and the real response shown inline for a failing assertion.
- **None (silent)**: no per-test output at all — just the final pass/fail
  summary line and exit code. Useful in CI logs where per-test noise isn't
  wanted but the run still needs to fail the build on a non-zero exit.

## Streaming NDJSON (`--stream`)

```bash
grpctestify tests/ --stream
```

Emits one JSON object per line to stdout — for an IDE or CI system that wants
to react to progress as it happens instead of parsing a file after the run
finishes. Event shapes:

```jsonc
{"event":"suite_start","testCount":12,"timestamp":"2026-07-22T12:00:00Z"}
{"event":"test_start","testId":"users.Get.gctf","timestamp":"..."}
{"event":"test_pass","testId":"users.Get.gctf","duration":8,"grpcDuration":5,"timestamp":"..."}
{"event":"test_fail","testId":"users.List.gctf","duration":42,"message":"1 assertion failed","assertions":[{"line":5,"expression":".total == 3","passed":false,"expected":"3","actual":"2"}],"timestamp":"..."}
{"event":"suite_end","summary":{"total":12,"passed":11,"failed":1,"skipped":0,"duration":812},"timestamp":"..."}
```

`test_pass`/`test_fail`/`test_skip` carry `assertions` (line/expression/
passed, plus expected/actual/message on failure) whenever the test recorded
any — the same per-assertion detail the verbose console and other reporters
show, without a second pass over the results.

## Exit codes

Every form agrees on the same convention: `0` if every test passed, `1` if
any test failed (or the test set was empty — see [Notes](./report-formats#notes)).
`--stream`/`--progress none` change what's printed, not the exit code.

## NO_COLOR

All styled console output (icons, pass/fail colors, dim secondary text)
respects [`NO_COLOR`](https://no-color.org/) — set it to disable ANSI color
regardless of TTY detection. Streaming NDJSON and file-based report formats
never contain ANSI codes to begin with.
