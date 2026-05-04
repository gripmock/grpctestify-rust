# Attributes

::: warning Experimental
Attributes are an **experimental feature**.
The syntax, behavior, and available attributes may change in future releases
without a migration path.
This feature may be removed entirely in a future version.
:::

Per-section execution modifiers using `#[name(value)]` syntax.

## When to use

- Skip a section during execution (`#[skip]`)
- Set per-section timeout (`#[timeout(10)]`)
- Retry a section on failure (`#[retry(3)]`)
- Override display name in reports (`#[name(Login flow)]`)

## Syntax

Attributes are placed on a separate line immediately before a section header:

```gctf
#[skip]
--- REQUEST ---
{
  "user_id": "123"
}
```

Multiple attributes can be applied to one section:

```gctf
#[timeout(10)]
#[retry(2)]
--- REQUEST ---
{
  "query": "slow search"
}
```

## Supported attributes

| Attribute | Value | Description |
| --------- | ----- | ----------- |
| `#[skip]` | flag (or `#[skip(true)]`) | Skip this section during execution |
| `#[timeout(N)]` | seconds | Per-section timeout for gRPC request |
| `#[retry(N)]` | count | Retry section on failure with 100ms × attempt delay |
| `#[name(...)]` | string | Display name for this section in reports |

## Inheritance

Attributes inherit between sections. A child section can override a parent's attribute:

```gctf
#[timeout(30)]
--- REQUEST ---
{
  "user_id": "1"
}

--- RESPONSE ---
{
  "id": "1"
}

#[timeout(5)]
--- REQUEST ---
{
  "user_id": "2"
}
```

In this example:

- First `REQUEST` inherits `timeout(30)`
- Second `REQUEST` overrides with `timeout(5)`

## Skip behavior

`#[skip]` is a boolean flag. Both forms are equivalent:

```gctf
#[skip]
--- REQUEST ---
{}
```

```gctf
#[skip(true)]
--- REQUEST ---
{}
```

A skipped section is ignored during execution. The test continues with subsequent sections.

## Examples

### Retry flaky assertions

```gctf
--- ENDPOINT ---
user.UserService/GetUser

#[retry(3)]
--- REQUEST ---
{
  "user_id": "123"
}

--- ASSERTS ---
.status == "active"
```

### Timeout for slow endpoints

```gctf
--- ENDPOINT ---
search.SearchService/Search

#[timeout(60)]
--- REQUEST ---
{
  "query": "complex aggregation"
}
```

## Rules

- One attribute per line
- Attribute must appear on the line immediately before `--- SECTION ---`
- Attributes apply to the section that follows them
- Values are strings by default; numbers are parsed from the value
- Quoted strings are supported: `#[name("My Test")]`

## Related

- [META](./meta) — file-level metadata (tags, owner, summary)
- [OPTIONS](./options) — global execution options
- [Report Formats](../api/report-formats)
