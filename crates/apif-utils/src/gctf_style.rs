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
