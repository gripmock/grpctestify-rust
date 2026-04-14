// Commands module - handles CLI command execution

use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
use anyhow::Result;

pub mod check;
pub mod explain;
pub mod fmt;
pub mod inspect;
pub mod list;
pub mod lsp;
pub mod reflect;
pub mod run;

pub use check::handle_check;
pub use explain::handle_explain;
pub use fmt::handle_fmt;
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

/// Truncate string to max length with ellipsis
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
