// Assertion plugin: @luhn_valid(value)
//
// A realistic domain-specific check that has no built-in equivalent: Luhn
// checksum validation (credit card numbers, IMEI, some national ID
// schemes). Demonstrates a script doing real, non-trivial logic — exactly
// the kind of one-off validator custom plugins exist for.
//
// Usage in ASSERTS:
//   @luhn_valid(.card_number)

/// Validates a numeric string against the Luhn checksum (card numbers,
/// IMEI, some national ID schemes). Throws on a non-digit character.
/// @param value: string
/// @returns bool
/// @pure
fn luhn_valid(value) {
    let digits = value.to_string();
    let sum = 0;
    let alt = false;
    let i = digits.len() - 1;

    while i >= 0 {
        let ch = digits[i];
        if ch < '0' || ch > '9' {
            throw `luhn_valid: non-digit character '${ch}'`;
        }
        let d = ch.to_int() - '0'.to_int();
        if alt {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        alt = !alt;
        i -= 1;
    }

    sum % 10 == 0
}
