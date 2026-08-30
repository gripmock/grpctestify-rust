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
    fn ascii_roundtrip() {
        let line = "hello world";
        assert_eq!(utf16_col_to_byte(line, 0), 0);
        assert_eq!(utf16_col_to_byte(line, 6), 6);
        assert_eq!(byte_to_utf16_col(line, 6), 6);
        assert_eq!(utf16_col_to_byte(line, 100), line.len());
    }

    #[test]
    fn multibyte_bmp() {
        let line = "café x";
        assert_eq!(utf16_col_to_byte(line, 4), 5);
        assert_eq!(byte_to_utf16_col(line, 5), 4);
        assert_eq!(&line[utf16_col_to_byte(line, 4)..], " x");
    }

    #[test]
    fn multibyte_cyrillic() {
        let line = "Привет {{ x }}";
        let byte = utf16_col_to_byte(line, 7);
        assert_eq!(&line[byte..], "{{ x }}");
        assert_eq!(byte_to_utf16_col(line, byte), 7);
    }

    #[test]
    fn astral_char() {
        let line = "😀ab";
        assert_eq!(utf16_col_to_byte(line, 2), 4);
        assert_eq!(byte_to_utf16_col(line, 4), 2);
        assert_eq!(&line[utf16_col_to_byte(line, 2)..], "ab");
    }
}
