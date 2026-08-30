use anyhow::Result;
use clap::{CommandFactory, FromArgMatches};
use tracing::info;

use grpctestify::cli;
use grpctestify::commands;

use cli::{Cli, Commands};

fn help_logo() -> String {
    let g = grpctestify::report::style::pass_style();
    grpctestify::report::style::SNAIL_LOGO
        .lines()
        .map(|line| g.apply_to(line).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_worker_threads(args: &[String]) -> usize {
    let parallelism = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let env = std::env::var("TOKIO_WORKER_THREADS").ok();
    worker_threads_for(args, env.as_deref(), parallelism)
}

fn worker_threads_for(args: &[String], env: Option<&str>, parallelism: usize) -> usize {
    let parallelism = parallelism.max(1);

    if let Some(n) = args
        .iter()
        .position(|a| a == "--cpus")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
    {
        return n;
    }
    if let Some(n) = env.and_then(|v| v.parse::<usize>().ok()).filter(|n| *n > 0) {
        return n;
    }
    if args.iter().any(|a| a == "bench") {
        return parallelism.min(BENCH_MAX_WORKER_THREADS);
    }
    parallelism
}

const BENCH_MAX_WORKER_THREADS: usize = 4;

fn main() -> Result<()> {
    grpctestify::report::set_tool_identity("grpctestify", env!("CARGO_PKG_VERSION"));
    let argv: Vec<String> = std::env::args().collect();
    let worker_threads = resolve_worker_threads(&argv);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

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

    let command = Cli::command().before_long_help(help_logo());
    let cli = match Cli::from_arg_matches(&command.get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };

    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    let base_filter = if cli.verbose {
        "grpctestify=info,warn"
    } else {
        "grpctestify=warn,error"
    };
    let is_play = matches!(cli.command, Some(Commands::Play(_)));
    let filter = if is_play {
        format!("{},tower_http=info", base_filter)
    } else {
        base_filter.to_string()
    };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&filter));

    let writer = if is_play {
        BoxMakeWriter::new(std::io::stdout)
    } else {
        BoxMakeWriter::new(std::io::stderr)
    };

    tracing_subscriber::fmt()
        .with_writer(writer)
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
        Some(Commands::Docs(args)) => commands::handle_docs(args),
        Some(Commands::Graph(args)) => commands::handle_graph(args),
        Some(Commands::Run(args)) => commands::run_tests(&cli, args).await,
        Some(Commands::Call(args)) => commands::handle_call(args).await,
        Some(Commands::Gen(args)) => commands::handle_gen(args).await,
        Some(Commands::Lsp(args)) => commands::handle_lsp(args).await,
        Some(Commands::Bench(args)) => commands::handle_bench(args).await,
        Some(Commands::BenchCompare(args)) => commands::bench_compare::run(args),
        Some(Commands::BenchAggregate(args)) => commands::bench_aggregate::run(args),
        Some(Commands::Query(args)) => commands::handle_query(args),
        Some(Commands::Health(args)) => commands::handle_health(args).await,
        Some(Commands::Play(args)) => commands::handle_play(args).await,
        Some(Commands::Scaffold(args)) => commands::handle_scaffold(args).await,
        Some(Commands::Plugins(args)) => {
            use cli::args::PluginsAction;
            let action = args.action.clone();
            tokio::task::spawn_blocking(move || match action {
                PluginsAction::Install(a) => commands::handle_plugins_install(&a),
                PluginsAction::List(a) => commands::handle_plugins_list(&a),
                PluginsAction::Remove(a) => commands::handle_plugins_remove(&a),
                PluginsAction::Update(a) => commands::handle_plugins_update(&a),
            })
            .await?
        }
        None => {
            let args = cli.run_args.clone();
            if args.test_paths.is_empty() {
                print_welcome();
                return Ok(());
            }
            commands::run_tests(&cli, &args).await
        }
    }
}

fn print_welcome() {
    use grpctestify::report::style::{bold_style, dim_style};
    let b = bold_style();
    let d = dim_style();
    let version = env!("CARGO_PKG_VERSION");

    let g = grpctestify::report::style::pass_style();
    for line in grpctestify::report::style::SNAIL_LOGO.lines() {
        println!("  {}", g.apply_to(line));
    }
    println!();

    println!("{} {}", b.apply_to("grpctestify"), d.apply_to(version));
    println!("Native gRPC testing with .gctf files — zero dependencies");
    println!();

    for (invocation, desc) in [
        ("grpctestify <path>", "run the .gctf tests under <path>"),
        ("grpctestify --help", "all commands and options"),
        ("grpctestify <command> --help", "help for one command"),
    ] {
        println!(
            "  {}  {}",
            b.apply_to(format!("{invocation:<28}")),
            d.apply_to(desc)
        );
    }
    println!();
    println!(
        "{} https://gripmock.github.io/grpctestify-rust/",
        d.apply_to("Docs:")
    );
}

#[cfg(test)]
mod tests {
    use super::worker_threads_for;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn bench_caps_worker_threads_below_the_host_core_count() {
        assert_eq!(
            worker_threads_for(&argv(&["grpctestify", "bench", "x.gctf"]), None, 64),
            4
        );
        assert_eq!(
            worker_threads_for(&argv(&["grpctestify", "bench", "x.gctf"]), None, 2),
            2
        );
    }

    #[test]
    fn other_commands_keep_full_parallelism() {
        assert_eq!(
            worker_threads_for(&argv(&["grpctestify", "run", "tests/"]), None, 64),
            64
        );
    }

    #[test]
    fn cpus_flag_wins_over_env_and_defaults() {
        let args = argv(&["grpctestify", "bench", "x.gctf", "--cpus", "7"]);
        assert_eq!(worker_threads_for(&args, Some("2"), 64), 7);
    }

    #[test]
    fn env_wins_over_the_default_but_not_the_flag() {
        let args = argv(&["grpctestify", "bench", "x.gctf"]);
        assert_eq!(worker_threads_for(&args, Some("2"), 64), 2);
    }

    #[test]
    fn nonsense_values_fall_through_rather_than_producing_zero_threads() {
        let args = argv(&["grpctestify", "bench", "x.gctf", "--cpus", "0"]);
        assert_eq!(worker_threads_for(&args, Some("nope"), 8), 4);
        let trailing = argv(&["grpctestify", "bench", "x.gctf", "--cpus"]);
        assert_eq!(worker_threads_for(&trailing, None, 8), 4);
        assert!(worker_threads_for(&argv(&["grpctestify", "run"]), None, 0) >= 1);
    }
}
