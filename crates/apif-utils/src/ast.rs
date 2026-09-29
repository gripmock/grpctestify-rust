pub fn section_content_line(start_line: usize, idx: usize) -> usize {
    start_line + idx + 2
}

pub fn section_header_line(start_line: usize) -> usize {
    start_line + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_content_line_first() {
        assert_eq!(section_content_line(5, 0), 7);
    }

    #[test]
    fn section_content_line_offset() {
        assert_eq!(section_content_line(10, 3), 15);
    }

    #[test]
    fn header_line_is_one_based() {
        assert_eq!(section_header_line(0), 1);
        assert_eq!(section_header_line(11), 12);
    }

    #[test]
    fn header_line_precedes_first_content_line() {
        assert_eq!(section_header_line(4) + 1, section_content_line(4, 0));
    }
}
