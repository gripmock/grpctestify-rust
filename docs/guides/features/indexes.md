# Index System

gRPC Testify builds persistent indexes for data source columns to enable fast row lookups during benchmark execution.

## How Indexes Work

When a data source has `indexed_by` configured, the CLI builds a **SourceIndex** — a binary file (`.gcti`) containing:

- **Key column name** and inferred **key type** (string, u64, i64, UUID, etc.)
- **Entry table** mapping each key to its byte offset and row length in the source file
- **CRC32 checksum** for corruption detection

## Index Commands

```bash
# Rebuild indexes for a data source
grpctestify index --rebuild data/users.csv --key user_id

# Force rebuild (ignore cache)
grpctestify index --rebuild data/users.csv --key user_id --force

# Show index statistics
grpctestify index --stats data/users.gcti
```

## Key Types

| Type | Detection | Storage |
| ---- | --------- | ------- |
| String | default | Sorted BTreeMap |
| U64 / I64 | numeric parse | Sorted Vec (binary search) |
| UUID | regex match | Packed 128-bit |
| ULID | Crockford base32 | Packed 128-bit |
| DatePacked | `YYYY-MM-DD` | Packed u32 |
| TimePacked | `HH:MM:SS` | Packed u32 |

## Performance

Index lookup is O(log n) for numeric keys (binary search) and O(log n) for string keys (sorted BTreeMap):

- 500K entries: ~2µs per lookup
- Index file: ~50MB for 500K string keys
