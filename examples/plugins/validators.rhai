// One file, two independent assertion plugins — @is_even and @in_range.
// There's no single magic function name to build a file around; every
// public function is its own plugin, named after itself.
//
// Both are tagged @pure with full @param/@returns types: their logic is
// genuinely deterministic and side-effect-free, so it's honest to let the
// optimizer treat them the same way it treats a built-in like @is_uuid
// (e.g. rewriting `@is_even(x) == true` to `@is_even(x)`). Tags are a
// trust-based promise, not a proof — see docs/guides/plugins/custom-scripts.

/// Checks that a number is even.
/// @param value: number
/// @returns bool
/// @pure
fn is_even(value) {
    value % 2 == 0
}

/// Checks a value falls within [min, max], inclusive.
/// @param value: number
/// @param min: number
/// @param max: number
/// @returns bool
/// @pure
fn in_range(value, min, max) {
    value >= min && value <= max
}
