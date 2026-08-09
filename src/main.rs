use anyhow::Result;
use clap::{CommandFactory, FromArgMatches};
use tracing::info;

use grpctestify::cli;
use grpctestify::commands;

use cli::{Cli, Commands};

/// The mascot rendered for `--help`, coloured through the same `pass_style()`
/// path as the no-args welcome so the logo looks identical in both places
/// (green on a colour terminal, plain under `NO_COLOR` or when piped).
fn help_logo() -> String {
    let g = grpctestify::report::style::pass_style();
    grpctestify::report::style::SNAIL_LOGO
        .lines()
        .map(|line| g.apply_to(line).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `--cpus` wins, then `TOKIO_WORKER_THREADS`, else `bench` caps at 4 and every
/// other command keeps `available_parallelism`. More threads cost instructions
/// per request without adding throughput — see `docs/.../bench.md`.
fn resolve_worker_threads(args: &[String]) -> usize {
    let parallelism = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let env = std::env::var("TOKIO_WORKER_THREADS").ok();
    worker_threads_for(args, env.as_deref(), parallelism)
}

/// Pure half of [`resolve_worker_threads`], so the precedence is testable
/// without touching process state.
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

/// Above this, worker threads cost CPU and instructions per request without
/// adding throughput; the tax grows with the host's core count.
const BENCH_MAX_WORKER_THREADS: usize = 4;

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let worker_threads = resolve_worker_threads(&argv);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<()> {
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

    let command = Cli::command().before_long_help(help_logo());
    let cli = match Cli::from_arg_matches(&command.get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };

    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    // `-v` raises visibility to info (not debug) so verbose test output isn't
    // buried in gRPC debug traces. `grpctestify=debug`/`=trace` only scopes to
    // this one crate — the `apif-*` workspace crates (grpc transport,
    // assertion engine, plugin loading, ...) have their own crate-name
    // targets and won't match it. For full debug detail (e.g. before filing
    // a bug report), use the bare `RUST_LOG=debug` or `RUST_LOG=trace` (no
    // crate scope) — it covers every first-party crate plus tonic/hyper
    // internals in one go, and is honoured first by the env filter below.
    let base_filter = if cli.verbose {
        "grpctestify=info,warn"
    } else {
        "grpctestify=warn,error"
    };
    let is_play = matches!(cli.command, Some(Commands::Play(_)));
    // In play mode, include HTTP access logs from tower_http
    let filter = if is_play {
        format!("{},tower_http=info", base_filter)
    } else {
        base_filter.to_string()
    };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&filter));

    // Logs must never share a stream with a command's primary payload — `gen`,
    // `inspect`, `query`, `explain`, etc. print their real output to stdout
    // for piping/redirection, and raising verbosity to debug a bug is exactly
    // when a user is most likely to be capturing that stdout for a report.
    // `play` is a long-running dev server with no piped payload of its own,
    // so its access/app logs stay on stdout (prior behavior, easier to `|
    // grep`/tail alongside the proxied traffic it's serving).
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
            // Implicit Run
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

    // Braille-art snail — the project mascot, shared with `--help`.
    let g = grpctestify::report::style::pass_style();
    for line in grpctestify::report::style::SNAIL_LOGO.lines() {
        println!("  {}", g.apply_to(line));
    }
    println!();

    // The no-args screen is the landing: identity + the few pointers a newcomer
    // needs. It deliberately does NOT list commands/options/examples — `--help`
    // owns the full reference, so the two never read as duplicates of each other.
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
        // A host smaller than the cap keeps its own core count.
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
        // A runtime with zero worker threads would not start.
        assert!(worker_threads_for(&argv(&["grpctestify", "run"]), None, 0) >= 1);
    }
}
