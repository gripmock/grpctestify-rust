use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

use grpctestify::cli;
use grpctestify::commands;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider (ring) to avoid panics with rustls 0.23+
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Honour NO_COLOR (https://no-color.org/) before any output, including clap --help.
    let disable_color = std::env::var_os("NO_COLOR").is_some();
    if disable_color {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let cli = Cli::parse();

    use tracing_subscriber::EnvFilter;

    // Access log (tower_http) goes to stdout, other logs to stderr
    let base_filter = if cli.verbose {
        "grpctestify=debug,warn"
    } else {
        "grpctestify=warn,error"
    };
    // In play mode, include HTTP access logs from tower_http
    let filter = if matches!(cli.command, Some(Commands::Play(_))) {
        format!("{},tower_http=info", base_filter)
    } else {
        base_filter.to_string()
    };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&filter));

    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .event_format(grpctestify::logging::CustomFormatter)
        .with_env_filter(env_filter)
        .init();

    if cli.verbose {
        info!("Starting grpctestify v{}", env!("CARGO_PKG_VERSION"));
    }

    if let Some(shell_type) = cli.completion {
        commands::handle_completion(&shell_type)?;
        return Ok(());
    }

    match &cli.command {
        Some(Commands::Reflect(args)) => commands::handle_reflect(args).await,
        Some(Commands::Fmt(args)) => commands::handle_fmt(args, &cli).await,
        Some(Commands::Check(args)) => commands::handle_check(args, &cli).await,
        Some(Commands::Explain(args)) => commands::handle_explain(args).await,
        Some(Commands::Grpcurl(args)) => commands::handle_grpcurl(args).await,
        Some(Commands::Inspect(args)) => commands::handle_inspect(args).await,
        Some(Commands::Index(args)) => commands::handle_index(args),
        Some(Commands::List(args)) => commands::handle_list(args),
        Some(Commands::Run(args)) => commands::run_tests(&cli, args).await,
        Some(Commands::Call(args)) => commands::handle_call(args).await,
        Some(Commands::Gen(args)) => commands::handle_gen(args).await,
        Some(Commands::Lsp(args)) => commands::handle_lsp(args).await,
        Some(Commands::Bench(args)) => commands::handle_bench(args).await,
        Some(Commands::BenchCompare(args)) => commands::bench_compare::run(args),
        Some(Commands::Query(args)) => commands::handle_query(args),
        Some(Commands::Health(args)) => commands::handle_health(args).await,
        Some(Commands::Play(args)) => commands::handle_play(args).await,
        Some(Commands::Scaffold(args)) => commands::handle_scaffold(args).await,
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
