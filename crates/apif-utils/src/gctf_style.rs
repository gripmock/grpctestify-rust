pub fn trailing_blank_line_count(lines: &[&str], start: usize, end: usize) -> usize {
    if start >= end || start >= lines.len() {
        return 0;
    }

    let upper = end.min(lines.len());
    let mut count = 0usize;
    for idx in (start..upper).rev() {
        if lines[idx].trim().is_empty() {
            count += 1;
        } else {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trailing_blank_line_count_no_blanks() {
        let lines = &["hello", "world"];
        assert_eq!(trailing_blank_line_count(lines, 0, 2), 0);
    }

    #[test]
    fn test_trailing_blank_line_count_with_blanks() {
        let lines = &["hello", "world", "", "  ", ""];
        assert_eq!(trailing_blank_line_count(lines, 0, 5), 3);
    }

    #[test]
    fn test_trailing_blank_line_count_edge_cases() {
        let lines = &["hello", ""];
        // start >= end
        assert_eq!(trailing_blank_line_count(lines, 3, 2), 0);
        // start >= len
        assert_eq!(trailing_blank_line_count(lines, 5, 10), 0);
        // end clamped
        assert_eq!(trailing_blank_line_count(lines, 0, 10), 1);
    }
}
