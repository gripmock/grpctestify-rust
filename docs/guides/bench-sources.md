# Data Sources for Load Tests

Load test parameterization from external data files (CSV, TSV, NDJSON). Each row becomes a gRPC request with template substitution.

## When to Use

Use data sources when you need to:
- Test with thousands of different input combinations
- Load test with realistic data variations
- Parameterize requests from external databases or generated data

## Basic Example

```yaml
--- BENCH ---
sources:
  - name: users
    file: data/users.csv

--- REQUEST ---
{
  "user_id": "{{users.id}}",
  "name": "{{users.name}}"
}
```

Each row from `users.csv` produces one gRPC request.

## Source Definition

```yaml
--- BENCH ---
sources:
  - name: my_source          # Template name (optional, defaults to filename)
    file: data/file.csv       # Path to data file (required)
    format: csv               # csv, tsv, ndjson (auto-detected)
    indexed_by: user_id       # Column for fast lookups
```

| Field | Description |
|-------|-------------|
| `name` | Source name for templates like `{{name.column}}` |
| `file` | Path to data file (relative to `.gctf`) |
| `format` | `csv`, `tsv`, `ndjson` (auto-detected from extension) |
| `indexed_by` | Column for FK lookups (speeds up `{{source.column}}` joins) |

## Supported File Formats

| Format | Extension | Example |
|--------|----------|---------|
| CSV | `.csv` | `id,name,age\n1,alice,25` |
| TSV | `.tsv` | `id\tname\tage` |
| NDJSON | `.ndjson`, `.jsonl` | `{"id":1,"name":"alice"}` |

### CSV Options

```yaml
sources:
  - name: users
    file: data/users.csv
    delimiter: ";"      # Use semicolon instead of comma
```

## Template Variables

Use `{{source_name.column}}` in any section:

```yaml
--- BENCH ---
sources:
  - name: pvz
    file: data/pvz.csv
  - name: regions
    file: data/regions.csv
    indexed_by: region_id

--- REQUEST ---
{
  "pvz_id": "{{pvz.id}}",
  "region": "{{regions.name}}"    # FK lookup via indexed column
}
```

### How It Works

1. **Primary source** — rows read sequentially, one per request
2. **Dimension sources** — looked up by `indexed_by` column value from primary row
3. **All columns** — available as `{{source.column}}` variables

## Building Indexes

Index files (`.gcti`) speed up FK lookups. Build them before running bench:

```bash
# Index all sources in a test file
grpctestify index test.gctf

# Rebuild even if up-to-date
grpctestify index test.gctf --force

# Index all .gctf files in a directory
grpctestify index ./benchmarks/
```

Indexes are rebuilt automatically when source files change.

## Row Filters

Reduce data before loading:

```yaml
sources:
  - name: users
    file: data/users.csv
    filter:
      - field: status
        equals: active
      - field: age
        gte: 18
```

**Operators:** `equals`, `in`, `gte`, `lt`

## Relationships Between Sources

### N:1 — Primary to Dimension

```yaml
--- BENCH ---
sources:
  - name: orders
    file: data/orders.csv           # Primary: one row per request
  - name: customers
    file: data/customers.csv
    indexed_by: customer_id         # Lookup key
```

```yaml
--- REQUEST ---
{
  "order_id": "{{orders.id}}",
  "customer_name": "{{customers.name}}"   # FK → dimension
}
```

### N:M — Via Mapping Table

```yaml
--- BENCH ---
sources:
  - name: orders
    file: data/orders.csv
  - name: products
    file: data/products.csv
    indexed_by: product_id
  - name: order_products
    file: data/order_products.csv
    indexed_by: order_id,product_id
```

## Troubleshooting

### "Index hint" warning

Build indexes when `explain` or `inspect` suggests it:

```bash
grpctestify index test.gctf
```

### Slow FK lookups

1. Ensure `indexed_by` column exists on dimension sources
2. Rebuild index: `grpctestify index test.gctf --force`
3. Check file sizes — smaller files load faster into memory

### Out of memory

Lower memory budget for indexes:

```bash
GRPCTESTIFY_DIMENSION_MEMORY_BUDGET=256mb grpctestify bench test.gctf
```

## Related

- [BENCH Section](../sections/bench)
- [Template Variables](../sections/extract)
- [Query Command](./query) — explore sources interactively
