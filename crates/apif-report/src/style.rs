use console::{Style, style};

/// The project mascot — a Braille-art snail. Shared by the no-args welcome
/// screen and `--help` so the two present one identity. Lines start with the
/// Braille blank (U+2800), not ASCII space, so callers can indent by prefixing
/// without a string-continuation stripping the leading whitespace.
pub const SNAIL_LOGO: &str = "⠀⠀⠀⠀⣠⡄⠀⢠⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⣿⡇⠀⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⣿⡇⠀⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⣿⡇⠀⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⣀⣠⣤⣶⣶⣶⣶⣤⣄⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⢀⣴⣾⠿⠿⠿⠿⣿⣿⡀⠀⠀⠀⠀⣠⣶⡿⠿⠛⠉⠉⠉⠉⠉⠉⠛⠿⣷⣦⠀⠀⠀⠀⠀⠀⠀⠀
⣾⡿⠁⠀⠀⣤⣄⠈⢻⣷⠀⠀⣠⣾⠟⠋⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠱⣿⠆⠀⠀⠀⠀⠀⠀
⢿⣷⡀⠀⠀⠈⠁⠀⠘⣿⣇⣼⡿⠋⠀⠀⠀⠀⠀⢀⣠⣤⣤⣤⣤⡀⠀⠀⠀⠐⣿⣇⠀⠀⠀⠀⠀
⠈⠻⢿⣷⣶⡄⠀⠀⠀⠘⣿⣿⣧⠀⠀⠀⠀⠀⣴⣿⠟⠋⠉⠉⠛⢿⣧⡀⠀⠀⢸⣿⡄⠀⠀⠀⠀
⠀⠀⠀⠀⢿⣧⠀⠀⠀⠀⠈⠛⠿⣷⣦⣤⣄⣸⣿⠁⢠⣦⠀⠀⠀⠘⣿⡇⠀⠀⢸⣿⡇⠀⠀⠀⠀
⠀⠀⠀⠀⠘⢿⣧⠀⠀⠀⠀⠀⠀⠀⠉⠙⠻⢿⣿⠐⠹⣿⣄⣀⣀⣼⡿⠃⠀⠀⣸⣿⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠈⢿⣷⣄⠀⠀⠀⠀⠀⠀⠀⠀⠘⣿⣆⠀⠉⠻⠿⠟⠋⠁⠀⢀⣴⣿⠃⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠙⢿⣷⣤⣀⠀⠀⠀⠀⠀⠀⠈⠿⣷⣦⣀⣀⣀⣀⣠⣶⡿⠟⠻⣿⣦⣀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠘⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠘⠻⠿⠿⡿⠿⠿⠛⠀⠀⠀⠀⠻⣿⣧⡀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⣼⣿⣃⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣈⣻⣿⣄
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛";

pub const PASS_ICON: &str = "✓";
pub const FAIL_ICON: &str = "✗";
pub const SKIP_ICON: &str = "○";
pub const WARN_ICON: &str = "⚠";

pub fn pass_style() -> Style {
    Style::new().green()
}

pub fn fail_style() -> Style {
    Style::new().red().bold()
}

pub fn skip_style() -> Style {
    Style::new().yellow()
}

pub fn warn_style() -> Style {
    Style::new().yellow()
}

pub fn dim_style() -> Style {
    Style::new().dim()
}

pub fn bold_style() -> Style {
    Style::new().bold()
}

pub fn pass_icon() -> impl std::fmt::Display {
    pass_style().apply_to(PASS_ICON)
}

pub fn fail_icon() -> impl std::fmt::Display {
    fail_style().apply_to(FAIL_ICON)
}

pub fn skip_icon() -> impl std::fmt::Display {
    skip_style().apply_to(SKIP_ICON)
}

pub fn warn_icon() -> impl std::fmt::Display {
    warn_style().apply_to(WARN_ICON)
}

pub fn header(text: &str) -> String {
    style(text).bold().to_string()
}

/// A dim horizontal rule — `width` repetitions of `ch`. Centralizes the
/// hand-rolled `dim_style().apply_to("─".repeat(n))` pattern so section rules
/// render consistently and honor `NO_COLOR`/non-TTY through the same `console`
/// styling as every other helper here.
pub fn rule(ch: char, width: usize) -> String {
    dim_style()
        .apply_to(ch.to_string().repeat(width))
        .to_string()
}

pub fn format_duration_ms(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.2}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

pub fn duration_flagged(ms: u64, threshold_ms: u64) -> String {
    let text = format_duration_ms(ms);
    if ms >= threshold_ms {
        warn_style().apply_to(text).to_string()
    } else {
        dim_style().apply_to(text).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_ms_scales_at_one_second() {
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(1000), "1.00s");
        assert_eq!(format_duration_ms(1500), "1.50s");
        assert_eq!(format_duration_ms(0), "0ms");
    }

    #[test]
    fn duration_flagged_contains_the_formatted_number_either_way() {
        assert!(duration_flagged(50, 500).contains("50ms"));
        assert!(duration_flagged(600, 500).contains("600ms"));
        assert!(duration_flagged(1200, 500).contains("1.20s"));
    }

    #[test]
    fn rule_repeats_the_char_width_times() {
        // Content is `width` copies of `ch` regardless of any (TTY-gated)
        // styling wrapper — assert the glyph count, not the ANSI.
        assert_eq!(rule('─', 5).matches('─').count(), 5);
        assert_eq!(rule('═', 80).matches('═').count(), 80);
        assert_eq!(rule('─', 0).matches('─').count(), 0);
    }

    #[test]
    fn icons_render_the_expected_glyph() {
        assert!(pass_icon().to_string().contains(PASS_ICON));
        assert!(fail_icon().to_string().contains(FAIL_ICON));
        assert!(skip_icon().to_string().contains(SKIP_ICON));
        assert!(warn_icon().to_string().contains(WARN_ICON));
    }
}
