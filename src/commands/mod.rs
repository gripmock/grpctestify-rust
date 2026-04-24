// Commands module - handles CLI command execution

use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
use anyhow::Result;

pub mod call;
pub mod check;
pub mod explain;
pub mod fmt;
pub mod gen_grpcurl;
pub mod grpcurl;
pub mod inspect;
pub mod list;
pub mod lsp;
pub mod reflect;
pub mod run;

pub use call::handle_call;
pub use check::handle_check;
pub use explain::handle_explain;
pub use fmt::handle_fmt;
pub use gen_grpcurl::handle_gen;
pub use grpcurl::handle_grpcurl;
pub use inspect::handle_inspect;
pub use list::handle_list;
pub use lsp::handle_lsp;
pub use reflect::handle_reflect;
pub use run::run_tests;

/// Print diagnostic to stderr
pub fn print_diagnostic(diagnostic: &Diagnostic) {
    let severity_str = match diagnostic.severity {
        DiagnosticSeverity::Error => "ERROR",
        DiagnosticSeverity::Warning => "WARNING",
        DiagnosticSeverity::Information => "INFO",
        DiagnosticSeverity::Hint => "HINT",
    };

    eprintln!(
        "[{}] {}: {}",
        severity_str,
        diagnostic.code.as_str(),
        diagnostic.message
    );

    if let Some(context) = &diagnostic.context {
        eprintln!("  {}", context);
    }

    if !diagnostic.suggestions.is_empty() {
        eprintln!();
        eprintln!("Suggestions:");
        for suggestion in &diagnostic.suggestions {
            eprintln!("  - {}", suggestion);
        }
    }
}

/// Handle shell completion
pub fn handle_completion(shell_type: &str) -> Result<()> {
    use clap::CommandFactory;
    use clap_complete::{Shell, generate};

    let shell = match shell_type.to_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" => Shell::PowerShell,
        _ => {
            anyhow::bail!(
                "Unsupported shell: {}. Supported: bash, zsh, fish, elvish, powershell",
                shell_type
            );
        }
    };

    let mut cmd = crate::cli::Cli::command();
    let name = cmd.get_name().to_string();
    let mut stdout = std::io::stdout();

    generate(shell, &mut cmd, name, &mut stdout);

    Ok(())
}

/// Truncate string to max length with ellipsis.
/// `max_len` must be >= 3; if the string fits, it is returned as-is.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if max_len < 3 || s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_short_string_unchanged() {
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn truncate_str_exact_length_unchanged() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long_string_truncated() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn truncate_str_max_len_less_than_3_returns_original() {
        // Must not panic; returns original string when max_len < 3.
        assert_eq!(truncate_str("hello", 2), "hello");
        assert_eq!(truncate_str("hello", 0), "hello");
    }
}
