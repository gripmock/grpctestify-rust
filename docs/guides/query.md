# Query Command

Interactive shell and CLI for querying data from CSV, TSV, NDJSON files with format conversion.

## Usage

```bash
# Interactive shell mode
grpctestify query

# CLI mode with query
grpctestify query -q "users status=active" data/users.csv

# With stdin input
cat data.csv | grpctestify query -q "stdin status=active" -

# Output to file (format auto-detected from extension)
grpctestify query -q "users status=active" data/users.csv --output active_users.ndjson
```

## Query Syntax

```
<source> [col<op>value]...
```

Where:
- `<source>` — file name without extension, or `stdin`
- `col<op>value` — filter expression

### Filter Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `=` | Equals | `status=active` |
| `!=` | Not equals | `status!=pending` |
| `>=` | Greater or equal | `age>=18` |
| `<=` | Less or equal | `age<=65` |
| `>` | Greater | `score>100` |
| `<` | Less | `score<50` |
| `~glob` | Glob pattern | `name~glob"*John*"` |
| `~re:` | Regex | `msg~re:"error\|warn"` |
| `IN` | In list (comma-separated) | `status=active,pending` |

### Examples

```bash
# Simple equality
grpctestify query -q "users status=active" users.csv

# Multiple filters (AND)
grpctestify query -q "users status=active age>=18" users.csv

# Glob pattern
grpctestify query -q 'users name~glob"*John*"' users.csv

# Regex
grpctestify query -q 'logs msg~re:"error|warn|debug"' logs.ndjson

# IN list
grpctestify query -q "users status=active,pending,suspended" users.csv
```

## CLI Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--query` | `-q` | Query expression | - |
| `--shell` | `-s` | Force interactive shell | false |
| `--indexed-by` | `-i` | Index column | - |
| `--format` | `-f` | Output format (json, csv, table, line, tsv) | table |
| `--limit` | `-n` | Max rows to return | 100 |
| `--offset` | `-o` | Skip N rows | 0 |
| `--columns` | `-c` | Output columns (comma-separated) | all |
| `--order-by` | | Sort column (prefix `-` for DESC) | - |
| `--output` | | Output file (format from extension) | stdout |
| `--no-header` | | Skip header row in output | false |

## Output Formats

```bash
# Table (default)
grpctestify query -q "users id=1" users.csv
# ┌─────┬───────┬──────────┐
# │ id  │ name  │ status   │
# ├─────┼───────┼──────────┤
# │ 1   │ alice │ active   │
# └─────┴───────┴──────────┘

# JSON
grpctestify query -q "users id=1" users.csv -f json
# {"id":"1","name":"alice","status":"active"}

# CSV
grpctestify query -q "users id=1" users.csv -f csv
# id,name,status
# 1,alice,active

# Line (single value per row)
grpctestify query -q "users id=1" users.csv -f line
# 1,alice,active
```

## Interactive Shell

```bash
grpctestify query users.csv another.tsv
```

### Shell Commands

| Command | Description |
|---------|-------------|
| `.help` | Show help |
| `.quit` | Exit shell |
| `.tables` | List loaded sources |
| `.schema <name>` | Show source columns |
| `.indexes <name>` | Show index info |
| `.info` | Show all sources summary |
| `.count <name>` | Count rows |
| `.sample <name> [n]` | Show sample rows (default: 5) |
| `.load <file.gctf>` | Load sources from .gctf file |
| `.add <name> <file>` | Add file as source |
| `.remove <name>` | Remove source |
| `.mode <format>` | Set output format |
| `.headers <on\|off>` | Toggle headers |
| `.output <file>` | Set output file |

### Shell Example

```
$ grpctestify query users.csv
grpctestify query shell
Type '.help' for available commands

query> .tables
  users

query> .schema users
Schema for 'users':
  id
  name
  status
  age

query> .info
Sources: 1
  users: 4 columns

query> status=active
┌─────┬───────┬──────────┐
│ id  │ name  │ status   │
├─────┼───────┼──────────┤
│ 1   │ alice │ active   │
│ 3   │ charlie │ active │
└─────┴───────┴──────────┘

query> .quit
```

## Supported Formats

| Format | Extensions | Auto-detect |
|--------|------------|-------------|
| CSV | `.csv` | By content (comma-separated, no tabs) |
| TSV | `.tsv` | By content (tab-separated) |
| NDJSON | `.ndjson`, `.jsonl` | By content (lines starting with `{`) |
| JSON | `.json` | By extension only |

### Format Detection

When reading from stdin (`-`), format is auto-detected:
- Lines contain tabs but no commas → TSV
- Lines start with `{` → NDJSON
- Lines contain commas → CSV

## File Loading

Files are loaded by extension:
- `.gctf` — parsed as gRPC test file, sources extracted
- `.csv`, `.tsv`, `.ndjson`, `.json` — data file, source name = file stem

Directories can be specified — all `.gctf` files are loaded.

```bash
# Load directory of gctf files
grpctestify query ./tests/data/

# Multiple files
grpctestify query users.csv orders.csv products.ndjson
```
