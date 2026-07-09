# Tools

## Bench data generator

This generator is a standalone Rust tool. It is not part of the `grpctestify` binary.

Location:

- `tools/bench-data-gen/Cargo.toml`
- `tools/bench-data-gen/src/main.rs`

Run from repository root.

Generate all tiers into default output (`.tmp/bench-data`):

```bash
cargo run --manifest-path tools/bench-data-gen/Cargo.toml --release -- .tmp/bench-data
```

Generate one tier only (faster local check):

```bash
cargo run --manifest-path tools/bench-data-gen/Cargo.toml -- .tmp/bench-data 1mb
```

Supported tiers:

- `1kb`
- `128kb`
- `512kb`
- `1mb`
- `4mb`
- `100mb`
- `1gb`

Generated data includes all supported source formats:

- `csv`
- `tsv`
- `ndjson`

Each tier includes relationship patterns used in index/bench scenarios:

- `N:1` (e.g. `depots -> service_zones`)
- `1:N` (e.g. `customers -> shipments`)
- `N:M` / `M:N` (via `customer_items` + `catalog_items`)

Quick workflow for contributors:

1. Generate datasets:

```bash
cargo run --manifest-path tools/bench-data-gen/Cargo.toml --release -- .tmp/bench-data
```

1. Build/verify indexes for one scenario:

```bash
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_csv.gctf
```

1. Run bench scenario:

```bash
cargo run --bin grpctestify -- bench .tmp/bench-data/1mb/bench/index_matrix_csv.gctf
```

## Index benchmark matrix

Run index flow for all formats on one tier:

```bash
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_csv.gctf
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_tsv.gctf
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_ndjson.gctf
```

Validate reuse and forced rebuild:

```bash
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_csv.gctf
cargo run --bin grpctestify -- index .tmp/bench-data/1mb/bench/index_matrix_csv.gctf --force
```

What to check:

- first run: `Rebuilt: N` where `N > 0`
- second run without `--force`: `Rebuilt: 0`
- `--force` run: `Rebuilt: N` again
- `.gcti` files created in `.tmp/bench-data/<tier>/data/`
