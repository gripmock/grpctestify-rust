//! Position conversion helpers between LSP UTF-16 character offsets and Rust
//! byte offsets.
//!
//! The LSP protocol expresses character positions as UTF-16 code-unit offsets
//! within a line, but Rust `str` slicing uses byte offsets. Slicing a string
//! with a UTF-16 offset as if it were a byte offset panics on non-ASCII text
//! and produces misplaced ranges after multibyte characters. These helpers do
//! the conversion correctly by walking `char_indices` and summing UTF-16 code
//! unit lengths.

/// Convert an LSP UTF-16 character offset within `line` to a byte index.
///
/// The returned index always lands on a `char` boundary, so it is safe to use
/// for slicing `line`. Offsets past the end of the line clamp to `line.len()`.
pub fn utf16_col_to_byte(line: &str, utf16_col: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        if utf16_count >= utf16_col {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    line.len()
}

/// Convert a byte index within `line` to an LSP UTF-16 character offset.
///
/// A byte index that falls in the middle of a multibyte character counts the
/// characters strictly before it.
pub fn byte_to_utf16_col(line: &str, byte_idx: usize) -> usize {
    let mut utf16_count = 0usize;
    for (b, ch) in line.char_indices() {
        if b >= byte_idx {
            break;
        }
        utf16_count += ch.len_utf16();
    }
    utf16_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_roundtrip() {
        let line = "hello world";
        assert_eq!(utf16_col_to_byte(line, 0), 0);
        assert_eq!(utf16_col_to_byte(line, 6), 6);
        assert_eq!(byte_to_utf16_col(line, 6), 6);
        // Past the end clamps.
        assert_eq!(utf16_col_to_byte(line, 100), line.len());
    }

    #[test]
    fn test_multibyte_bmp() {
        // "café" — 'é' is 2 bytes in UTF-8 but 1 UTF-16 code unit.
        let line = "café x";
        // UTF-16 col 4 is the space after 'é'; byte index is 5 (c,a,f=3 + é=2).
        assert_eq!(utf16_col_to_byte(line, 4), 5);
        assert_eq!(byte_to_utf16_col(line, 5), 4);
        // Slicing at the converted byte index must not panic.
        assert_eq!(&line[utf16_col_to_byte(line, 4)..], " x");
    }

    #[test]
    fn test_multibyte_cyrillic() {
        // Each Cyrillic char is 2 bytes / 1 UTF-16 unit.
        let line = "Привет {{ x }}";
        let byte = utf16_col_to_byte(line, 7); // start of "{{"
        assert_eq!(&line[byte..], "{{ x }}");
        assert_eq!(byte_to_utf16_col(line, byte), 7);
    }

    #[test]
    fn test_astral_char() {
        // "😀" is 4 bytes in UTF-8 and 2 UTF-16 code units (a surrogate pair).
        let line = "😀ab";
        assert_eq!(utf16_col_to_byte(line, 2), 4); // after the emoji
        assert_eq!(byte_to_utf16_col(line, 4), 2);
        assert_eq!(&line[utf16_col_to_byte(line, 2)..], "ab");
    }
}
