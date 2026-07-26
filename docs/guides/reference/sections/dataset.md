# DATASET

Inline data-driven test rows — a YAML list of row objects, right inside the `.gctf` file. Each row expands the file into one test case, with its fields available as `{{dataset.field}}` template variables.

## When to use

- A handful of rows (2-3) that belong next to the test they drive, for code review
- Data with nested structure that doesn't fit a CSV/TSV row cleanly
- You'd otherwise reach for `run --data <file>` but a separate file is overkill

For larger datasets, prefer `run --data <file>` (CSV/TSV/NDJSON) — see [Test File Format](../api/test-files). The two are mutually exclusive per run: a file with a `DATASET` section can't also be driven by `--data`.

## Minimal example

```gctf
--- ENDPOINT ---
users.UserService/GetUser

--- DATASET ---
- id: "1"
  name: Ada
- id: "2"
  name: Grace

--- REQUEST ---
{ "id": "{{dataset.id}}" }

--- RESPONSE ---
{ "id": "{{dataset.id}}", "name": "{{dataset.name}}" }
```

This runs as 2 tests — one per row — each with `dataset.id`/`dataset.name` substituted from that row.

## Rules

- Body is a YAML list; each row must be an object (`key: value` pairs) — a malformed DATASET is a parse error, not a silently-empty run
- Fields are namespaced `dataset.<field>` in templates — same `<source>.<column>` convention `--data` uses
- Only one `DATASET` per file
- Mutually exclusive with `--data` for that run — combining them is a hard error
- Not usable as a `BENCH.sources:` input — `BENCH` needs indexed/memory-budgeted access to potentially large external files, which an inline YAML block in the same file you're hand-editing assertions in isn't meant for
- `#` comments are preserved (it's YAML, not `.gctf`'s own comment syntax) — `fmt` won't rewrite them to `//`

## Related

- [REQUEST section](./request)
- [Test File Format](../api/test-files) — `run --data` for larger/external datasets
