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
