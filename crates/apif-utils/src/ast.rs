/// Convert a section-relative line index to an absolute line number.
///
/// `start_line` is the line where the section header begins.
/// `idx` is the 0-based index within the section content lines.
/// The `+ 2` accounts for: (1) the header line itself, (2) the blank/content separator.
pub fn section_content_line(start_line: usize, idx: usize) -> usize {
    start_line + idx + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_content_line_first() {
        assert_eq!(section_content_line(5, 0), 7);
    }

    #[test]
    fn test_section_content_line_offset() {
        assert_eq!(section_content_line(10, 3), 15);
    }
}
