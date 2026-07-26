// Arity overloading: two `flexible_match` definitions at different
// parameter counts. Rhai dispatches by argument count at the call site —
// `@flexible_match(.field)` calls the 1-arg form, `@flexible_match(.field,
// "^[A-Z]")` calls the 2-arg form. Both share one plugin identity: they're
// two ways of asking the same underlying question ("does this field look
// right?"), not two unrelated plugins that happen to share a name.
//
// Usage in ASSERTS:
//   @flexible_match(.id)                 -- just "is it set"
//   @flexible_match(.code, "^[A-Z]{3}$")  -- "is it set AND does it match"

/// Checks the value is present (not the Rhai unit value `()`).
/// @param value: json
/// @returns bool
fn flexible_match(value) {
    value != ()
}

/// Checks the value is present AND matches a regex — uses the shared
/// `regex_match` stdlib helper (see docs/guides/plugins/stdlib) instead of
/// hand-rolling pattern matching.
/// @param value: string
/// @param pattern: string
/// @returns bool
fn flexible_match(value, pattern) {
    value != () && regex_match(value.to_string(), pattern)
}
