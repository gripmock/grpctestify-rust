# META

File-level metadata for readability, ownership, and test filtering.

## When to use

- Add human-friendly context (`name`, `summary`)
- Tag tests for selection (`--tags`, `--skip-tags`)
- Track ownership and useful links

## Minimal example

```gctf
--- META ---
name: get user success path
summary: Validates unary GetUser response and metadata
tags: [smoke, user]
owner: backend-qa
links:
  - https://example.org/specs/get-user
```

## Rules

- Optional section
- At most one `META` per document
- Must be the first section if present

## Fields

- `name`
- `summary`
- `tags`
- `owner`
- `links`

## Attribute-form fallback

Without a `META` section, `tags`/`owner`/`summary` can be set via
`#[tag(...)]`/`#[owner(...)]`/`#[summary(...)]` attributes on any section — used only for whichever of
the three `META` doesn't already provide:

```gctf
#[tag(smoke,user)]
#[owner(backend-qa)]
--- REQUEST ---
{}
```

See [Attributes](./attributes) for the full attribute list.

## Related

- [Command Line](../api/command-line)
- [Test File Format](../api/test-files)
- [Attributes](./attributes)
