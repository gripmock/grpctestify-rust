# Custom Assertion Plugins (`.rhai` scripts)

Drop a `.rhai` file into one of two convention directories and it's registered automatically at startup,
based on which function(s) it defines — no flag, no separate subdirectory per kind:

- `~/.grpctestify/plugins` — user-global, applies to every project
- `./.grpctestify/plugins` — project-local, the same `.grpctestify/` `play --init` creates

Neither directory is created for you — plugins only load from one that already exists. If a name is defined
in both, the project-local one wins.

```bash
mkdir -p .grpctestify/plugins
cp validators.rhai .grpctestify/plugins/
grpctestify run tests/
```

Supported on `run`, `check`, `fmt`, `explain`, and `inspect` — all five resolve the same two directories,
so a custom plugin is never flagged as unknown in one command but not another.

See also: [Reporter plugins](reporters) (the other thing a `.rhai` script can be) and
[the script stdlib](stdlib) (helpers every script gets for free).

## Every public function is a plugin

Every public top-level function in a script becomes its own assertion plugin, callable from `ASSERTS` under
its own name — `fn is_even(x) {...}` becomes `@is_even(...)`. One file can define many; there's no single
magic function name like `check` to build around, and the file it lives in isn't part of the plugin's name.

```rhai
// .grpctestify/plugins/validators.rhai
fn is_even(value) {
    value % 2 == 0
}

fn in_range(value, min, max) {
    value >= min && value <= max
}
```

```gctf
--- ASSERTS ---
@is_even(.count)
@in_range(.age, 18, 65)
```

## Arity dispatch

A function takes one named parameter per call argument — dispatch is by arity, same as Rhai's own function
overloading. A script can define more than one overload of the same name to serve more than one call shape
(e.g. `fn is_positive(x)` and `fn is_positive(x, y)` in the same file) — Rhai picks the one matching the number
of arguments at the call site. Calling a plugin with an argument count that doesn't match any overload is
a clear error naming the plugin and how many arguments it was called with.

## `private fn` stays internal

`private fn` (Rhai's own visibility keyword) keeps a helper internal — used by other functions in the file,
never itself exposed as a plugin:

```rhai
private fn threshold() { 200 }   // not a plugin

fn over_threshold(x) { x > threshold() }   // -> @over_threshold(...)
```

## Name conflicts

Two public functions sharing a name — same file, different files, or across the two convention directories —
is a load-time conflict, not "last one wins": the more specific source wins (project-local over user-global;
within one directory, the first file loaded in sorted order), and every other conflicting definition is
skipped with a logged error naming the sources involved. Pick distinct names.

## Doc comments

A `///` doc comment immediately above a function becomes that plugin's description (surfaced wherever built-in
plugin descriptions are — `explain`, diagnostics, and eventually LSP hover):

```rhai
/// Checks that a value is a positive number.
fn is_positive(x) { x > 0 }
```

## Type tags (optional, trust-based)

A doc comment can also carry `@param <name>: <type>`, `@returns <type>`, and `@pure` tags — `<type>` is one
of `bool`/`uint`/`number`/`string`/`time`/`json`/`yaml`, case-insensitive:

```rhai
/// Checks a value is within [min, max].
/// @param value: number
/// @param min: number
/// @param max: number
/// @returns bool
/// @pure
fn in_range(value, min, max) { value >= min && value <= max }
```

These aren't verified against the script body — a script author can claim `@pure` or the wrong return type
and nothing here catches it. It's the same trust boundary `unsafe` is in Rust: a promise, not a proof. What
they unlock:

- `@param`/`@returns` feed `explain`/diagnostics with real types instead of the default `Any`, and will feed
  LSP signature help once that lands.
- `@pure` is the one that matters for the optimizer: only a `@pure`-tagged plugin with `@returns bool` becomes
  eligible for a rewrite like `@is_positive(x) == true` → `@is_positive(x)`, the same as a built-in plugin with
  known `purity`/`return_type`. Without `@pure`, a plugin keeps the fully conservative defaults (never used as
  the basis for a rewrite) — that's still the default for every untagged function.

Untagged parameters/return default to `Any`, same as before this existed.

## Return values

- Returning a `bool` makes the plugin behave like `@is_uuid` — pass/fail.
- Returning anything else makes it a value-producing plugin like `@len` — composable, e.g. `@double(.x) == 84`.

## Constraints

- Pure functions only: no access to the response, headers, trailers, or the network. A script that fails to
  compile is skipped with a logged error; it does not stop the other scripts in the directory from loading.
- Sandboxed: every script engine runs with finite limits (operation count, string/array/map size), Rhai's
  built-in `eval` disabled, and no module resolver — a buggy or malicious script can't hang or OOM the `run`
  process, can't dynamically execute a string as more code, and can't `import` another file. The limits are
  generous for any real validator/reporter logic.
- Confirmed before it runs: see [Trusting a script](#trusting-a-script) below.
- LSP support: completion, hover, signature help, and inlay-hint type inference all know about
  convention-directory plugins, the same as built-ins. One limitation — the plugin list loads once when the
  server starts, so a script added or changed mid-session needs an editor/LSP restart to be picked up.
- Optimizer participation is opt-in via `@pure` (see above) — without it, custom plugins are never used as
  the basis for a rewrite.

## Trusting a script

A script in a convention directory runs with your privileges, and Rhai evaluates a script's top-level
statements as well as the function you called. A `.grpctestify/plugins/` directory is part of a repository,
so cloning someone else's project would otherwise be enough to execute their code.

The first time a script is about to run, grpctestify shows its path and sha256 and asks once:

```text
grpctestify wants to execute a script plugin:
  ./.grpctestify/plugins/my_checks.rhai
  sha256 9f2c...
It runs with your privileges. Only allow scripts you have read.
Execute it? [y/N]
```

Answering `y` records the hash in `~/.grpctestify/trusted_plugins.json` and later runs are silent. Editing
the script changes its hash, so it is asked again — an approval covers the exact contents you approved, not
the file name.

`check`, `inspect` and the LSP only need the names a script defines, so they compile it without executing it
and never ask.

Non-interactive runs (CI, a pipe, an editor task) can't answer, so they refuse to execute the script and log
why. Two environment variables settle it up front:

| Variable | Effect |
| --- | --- |
| `GRPCTESTIFY_TRUST_PLUGINS=1` | Execute script plugins without asking. Use in CI, where the checkout is already trusted. |
| `GRPCTESTIFY_NO_PLUGINS=1` | Never execute script plugins, even approved ones. Wins over the variable above. |

## Related

- [Plugin System overview](index)
- [Installing from a git host](installing)
- [Reporter plugins](reporters)
- [Script stdlib](stdlib)
