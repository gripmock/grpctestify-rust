// Main entry point for grpctestify

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

// Import from commands module
use grpctestify::cli;
use grpctestify::commands;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider (ring) to avoid panics with rustls 0.23+
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Handle --version manually for backward compatibility
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let cli = Cli::parse();

    // Setup tracing
    let filter = if cli.verbose {
        "grpctestify=debug,warn"
    } else {
        "grpctestify=warn,error"
    };

    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .event_format(grpctestify::logging::CustomFormatter)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .init();

    if cli.verbose {
        info!("Starting grpctestify v{}", env!("CARGO_PKG_VERSION"));
    }

    // Handle completion flag
    if let Some(shell_type) = cli.completion {
        commands::handle_completion(&shell_type)?;
        return Ok(());
    }

    match &cli.command {
        Some(Commands::Reflect(args)) => commands::handle_reflect(args).await,
        Some(Commands::Fmt(args)) => commands::handle_fmt(args).await,
        Some(Commands::Check(args)) => commands::handle_check(args).await,
        Some(Commands::Explain(args)) => commands::handle_explain(args).await,
        Some(Commands::Inspect(args)) => commands::handle_inspect(args).await,
        Some(Commands::List(args)) => commands::handle_list(args),
        Some(Commands::Run(args)) => commands::run_tests(&cli, args).await,
        Some(Commands::Lsp(args)) => commands::handle_lsp(args).await,
        None => {
            // Implicit Run
            let args = cli.run_args.clone();
            if args.test_paths.is_empty() {
                warn!("No test files provided. Use 'grpctestify --help' for usage.");
                return Ok(());
            }
            commands::run_tests(&cli, &args).await
        }
    }
}
