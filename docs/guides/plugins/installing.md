# Installing Plugins From a Git Host

`grpctestify plugins install` fetches a `.rhai` plugin straight from GitHub, GitLab, Bitbucket, or any git host — no shell-out to `git`, no central registry.

```bash
grpctestify plugins install github.com/owner/repo          # project-local: ./.grpctestify/plugins
grpctestify plugins install -g github.com/owner/repo       # user-global: ~/.grpctestify/plugins
```

## Source syntax

```
host/owner/repo[/subpath][@spec]
```

- `https://` prefix and a trailing `.git` are both accepted and stripped.
- `/subpath` points at a directory inside the repo if the `.rhai` files aren't at the root (non-recursive — only files directly in that directory are picked up, same as the hand-authored convention directories).
- `@spec` selects a version — see below.

```bash
grpctestify plugins install gitlab.com/owner/repo@v1.2.0
grpctestify plugins install github.com/owner/repo/plugins-dir@main
```

## `@spec` resolution

| Form | Meaning |
| --- | --- |
| (omitted) | The highest `vX.Y.Z`/`X.Y.Z` tag, or the default branch if the repo has no such tags |
| `@v1.2.3`, `@main`, `@abc1234` | **Exact literal ref** — matched as-is, never range-interpreted |
| `@^1.2.0`, `@~1.2.0`, `@>=1.0.0` | A semver range, matched against tag names |

A bare version like `@v1.2.3` is **exact**, not a caret range — this follows Cargo's git-dependency `tag = "..."` behavior, not npm's bare-version-means-caret default. There's no registry here to make "usually safe" a reasonable assumption, so reproducibility wins: what you pin is exactly what you get.

## What gets installed

Files land under `<plugins-dir>/installed/<host>/<owner>/<repo>/`, kept apart from hand-authored scripts directly under `<plugins-dir>`. A hand-authored plugin always wins over an installed one of the same name in the same tier.

A lockfile (`plugins.lock.yaml`, next to `plugins/`) records the requested spec, the resolved tag (if any), the resolved commit, and a sha256 per file. The checksum detects local drift or a corrupted re-fetch — it is **not** third-party-attested authenticity the way `sum.golang.org` is for Go modules; there's no registry to attest against. Trust here comes from reading the source before installing, the same as any other git dependency.

Every install/update prints the source, resolved commit, resolved version (if any), and the full file list — full provenance, no silent background changes. The fetch itself is grpctestify's own network activity; once installed, the script runs inside the same Rhai sandbox every plugin does (no `eval`, no network/filesystem access, operation and size limits) — installing from an arbitrary host grants no capability a hand-authored script doesn't already have.

## Optional repo-side manifest

A repo can drop a `grpctestify-plugin.yaml` in the installed directory (or `/subpath`) to add a bit of metadata — entirely optional, nothing breaks without it:

```yaml
description: Validates ISO 8601 date strings
grpctestify: ">=1.8"
files:
  - date_check.rhai
  - helpers/shared.rhai
```

- `description` — shown after install/update and in `plugins list`.
- `grpctestify` — a compat range (same `semver::VersionReq` syntax as `@spec` ranges) checked against the running `grpctestify` version. Informational only — a mismatch prints a warning, it never blocks the install.
- `files` — an explicit list of `.rhai` files to install (paths relative to the repo root, or to `/subpath` if one was given), overriding the default same-directory-only scan. Lets a repo point at nested files, or exclude ones (tests, examples) that shouldn't ship. Every listed path must exist and end in `.rhai`, or the install fails with a clear error — same trust posture as any other install-time check here.

There's deliberately no `version:` field — the git tag (or commit, for an untagged ref) already is the version, same reasoning as `@spec` resolution above. A `grpctestify-plugin.yaml` that fails to parse is treated as if it were absent (a warning is printed, install falls back to the default scan) — a mistake in someone else's optional manifest shouldn't block your install.

## Managing installed plugins

```bash
grpctestify plugins list [--all]        # --all shows both tiers
grpctestify plugins update [name]       # omit name to update everything in the tier
grpctestify plugins remove <name>       # name is host/owner/repo, as shown by `list`
```

`update` re-resolves each entry's original spec against the remote. An exact-ref pin is naturally a no-op unless the branch it points to moved; a range re-picks the current highest matching tag. Only entries whose resolved commit actually changed are re-fetched and reported — everything else reports "already up to date".

## Related

- [Plugin System overview](index)
- [Custom assertion plugins](custom-scripts)
- [Reporter plugins](reporters)
