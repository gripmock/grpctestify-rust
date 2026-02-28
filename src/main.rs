// Main entry point for grpctestify

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

// Import from commands module
use grpctestify::cli;
use grpctestify::commands;
use grpctestify::config;

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

    // Load configuration from file (if exists)
    let config = config::Config::load();

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

    // Handle config flag
    if cli.config {
        println!("Current configuration:");
        println!("\n  Command-line arguments:");
        let args = cli.get_run_args();
        println!("    Parallel jobs: {}", args.parallel);
        println!("    Sort mode: {}", args.sort);
        println!("    Timeout: {}s", args.timeout);
        println!(
            "    Retry: {} times, {}s delay",
            args.retry, args.retry_delay
        );
        if let Some(ref log_format) = args.log_format {
            println!("    Log format: {}", log_format);
        }
        if let Some(ref log_output) = args.log_output {
            println!("    Log output: {}", log_output.display());
        }

        if let Some(cfg) = config {
            println!("\n  Configuration file loaded:");
            if !cfg.general.address.is_empty() {
                println!("    Address: {}", cfg.general.address);
            }
            if !cfg.general.parallel.is_empty() {
                println!("    Parallel: {}", cfg.general.parallel);
            }
            if cfg.general.timeout != 0 {
                println!("    Timeout: {}s", cfg.general.timeout);
            }
            if cfg.general.retry != 0 {
                println!("    Retry: {} times", cfg.general.retry);
            }
            if cfg.general.retry_delay != 0.0 {
                println!("    Retry delay: {}s", cfg.general.retry_delay);
            }
            if let Some(ref log_format) = cfg.general.log_format {
                println!("    Log format: {}", log_format);
            }
            if let Some(ref log_output) = cfg.general.log_output {
                println!("    Log output: {}", log_output);
            }
            println!("    Progress mode: {}", cfg.progress.mode);
            println!(
                "  Color: {}",
                if cfg.progress.color {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            if cfg.coverage.enabled {
                println!("    Coverage: enabled");
                if let Some(ref output) = cfg.coverage.output {
                    println!("      Output: {}", output);
                }
            }
        } else {
            println!("\n  No configuration file loaded");
            println!("  Create one with: grpctestify --init-config .grpctestifyrc.toml");
        }

        println!("\n  Environment variables:");
        if let Ok(addr) = std::env::var(config::ENV_GRPCTESTIFY_ADDRESS) {
            println!("    {}: {}", config::ENV_GRPCTESTIFY_ADDRESS, addr);
        } else {
            println!(
                "    {}: not set (default: {})",
                config::ENV_GRPCTESTIFY_ADDRESS,
                config::default_address()
            );
        }

        println!("\nConfiguration precedence:");
        println!("  1. Command-line arguments (highest)");
        println!("  2. Configuration file");
        println!("  3. Environment variables");
        println!("  4. Built-in defaults (lowest)");

        return Ok(());
    }

    // Handle init_config flag
    if let Some(config_file) = cli.init_config {
        let config = config::Config::default();
        let toml_content = config.to_toml();
        std::fs::write(&config_file, toml_content)?;
        println!("Configuration file created: {}", config_file.display());
        println!("\nYou can now edit the file to customize your settings.");
        println!("\nConfiguration precedence:");
        println!("  1. Command-line arguments (highest)");
        println!("  2. Configuration file");
        println!("  3. Environment variables");
        println!("  4. Built-in defaults (lowest)");
        return Ok(());
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
