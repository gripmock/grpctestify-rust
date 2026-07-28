// Two more plugins from one file: @is_palindrome (bool) and @slugify
// (value-returning, not bool — composes with `==` the same way a built-in
// like @len does). `strip_dashes` is a `private fn` — a helper `slugify`
// uses internally that never becomes its own plugin. Neither is tagged
// @pure here on purpose: string manipulation like this is easy to get
// subtly wrong (unicode, locale), so leaving them at the conservative
// default is the more honest call for an example — @pure is for logic
// you're confident enough in to let the optimizer rely on.

/// Checks a string reads the same forwards and backwards.
/// @param value: string
/// @returns bool
fn is_palindrome(value) {
    let s = value.to_string().to_lower();
    let n = s.len();
    for i in range(0, n / 2) {
        if s[i] != s[n - 1 - i] {
            return false;
        }
    }
    true
}

// Not exposed as `@strip_dashes(...)` — `private fn` keeps it internal.
private fn strip_dashes(s) {
    let out = s;
    while out.len() > 0 && out[0] == '-' {
        out = out.sub_string(1);
    }
    while out.len() > 0 && out[out.len() - 1] == '-' {
        out = out.sub_string(0, out.len() - 1);
    }
    out
}

/// Turns a title into a URL-safe slug, e.g. "Hello World!" -> "hello-world".
/// @param value: string
/// @returns string
fn slugify(value) {
    let s = value.to_string().to_lower();
    let out = "";
    let prev_dash = false;
    for ch in s.chars() {
        if (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9') {
            out += ch;
            prev_dash = false;
        } else if !prev_dash {
            out += "-";
            prev_dash = true;
        }
    }
    strip_dashes(out)
}
