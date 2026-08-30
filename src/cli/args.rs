use clap::builder::styling::{AnsiColor, Styles};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::White.on_default());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Dots,
    None,
    Verbose,
}

impl std::str::FromStr for ProgressMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dots" => Ok(Self::Dots),
            "bar" => Ok(Self::Dots),
            "none" => Ok(Self::None),
            _ => Ok(Self::Dots),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogFormat {
    Console,
    Json,
    Yaml,
    JUnit,
    Allure,
    Html,
}

#[derive(Parser, Debug)]
#[command(name = "grpctestify")]
#[command(author = "grpctestify team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(styles = HELP_STYLES)]
#[command(
    about = "Native, zero-dependency gRPC testing with .gctf files",
    long_about = "Native, zero-dependency gRPC testing with .gctf files.\n\n\
        Run declarative .gctf tests against gRPC / gRPC-Web / Connect endpoints,\n\
        benchmark them, scaffold new tests from reflection, and explore APIs in a\n\
        web playground — all from a single self-contained binary.\n\n\
        Run `grpctestify` with no arguments for a quick tour.",
    after_help = "Examples:\n  \
        grpctestify tests/\n  \
        grpctestify tests/ --parallel 8 -v\n  \
        grpctestify reflect --address localhost:4770 --plaintext\n  \
        grpctestify scaffold --endpoint pkg.Svc/Method --reflect\n\n\
        Docs: https://gripmock.github.io/grpctestify-rust/"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub run_args: RunArgs,

    #[arg(short = 'v', long, global = true, default_value_t = false)]
    pub verbose: bool,

    #[arg(long = "optimize", short = 'O', value_name = "LEVEL", global = true, default_value_t = String::new())]
    pub optimize: String,

    #[arg(long, value_name = "SHELL_TYPE", value_parser = ["bash", "zsh", "fish", "elvish", "powershell"])]
    pub completion: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum Commands {
    Run(Box<RunArgs>),
    Check(CheckArgs),
    Fmt(FmtArgs),

    Scaffold(ScaffoldArgs),
    Gen(GenArgs),

    Reflect(ReflectArgs),
    Inspect(InspectArgs),
    Explain(ExplainArgs),
    Grpcurl(GrpcurlArgs),
    List(ListArgs),
    Docs(DocsArgs),
    Graph(GraphArgs),

    Call(CallArgs),
    Health(HealthArgs),

    Bench(BenchArgs),
    BenchCompare(BenchCompareArgs),
    BenchAggregate(BenchAggregateArgs),

    Index(IndexArgs),
    Query(QueryArgs),

    Play(PlayArgs),
    Lsp(LspArgs),

    Plugins(PluginsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub action: PluginsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PluginsAction {
    Install(PluginsInstallArgs),
    List(PluginsListArgs),
    Remove(PluginsRemoveArgs),
    Update(PluginsUpdateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsInstallArgs {
    pub source: String,

    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsListArgs {
    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,

    #[arg(long, default_value_t = false)]
    pub all: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsRemoveArgs {
    pub name: String,

    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsUpdateArgs {
    pub name: Option<String>,

    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ScaffoldArgs {
    #[arg(long, value_name = "SERVICE/METHOD", required = true)]
    pub endpoint: String,

    #[arg(long, value_name = "FILE_OR_DIR")]
    pub proto: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    pub descriptor: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub reflect: bool,

    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub force: bool,

    #[arg(long, default_value_t = false)]
    pub tls: bool,

    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,
}

#[derive(Args, Debug, Clone)]
pub struct HealthArgs {
    #[arg(required = true, value_name = "ADDRESS")]
    pub address: String,

    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    #[arg(long, value_name = "NAME")]
    pub service: Vec<String>,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    #[arg(long, default_value_t = false)]
    pub tls: bool,

    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    #[arg(long, default_value_t = 10, value_name = "SECS")]
    pub timeout: u64,

    #[arg(long, default_value_t = false)]
    pub watch: bool,

    #[arg(long, default_value_t = 1.0, value_name = "SECS")]
    pub interval: f64,

    #[arg(long, default_value_t = 60.0, value_name = "SECS")]
    pub watch_timeout: f64,
}

#[derive(Args, Debug, Clone)]
pub struct GrpcurlArgs {
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    #[arg(long)]
    pub doc_index: Option<usize>,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    #[arg(long, default_value_t = true)]
    pub stdio: bool,
}

#[derive(Args, Debug, Clone)]
pub struct BenchArgs {
    #[arg(long, value_name = "ADDRESS", conflicts_with = "calibrate")]
    pub address: Option<String>,

    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    #[arg(required = true, value_name = "PATH")]
    pub test_paths: Vec<PathBuf>,

    #[arg(long, value_name = "PROFILE")]
    pub profile: Option<String>,

    #[arg(long, value_name = "MODE")]
    pub mode: Option<String>,

    #[arg(short = 'c', long, value_name = "N")]
    pub concurrency: Option<u32>,

    #[arg(short = 'n', long, value_name = "N")]
    pub requests: Option<u64>,

    #[arg(short = 'd', long, value_name = "DURATION")]
    pub duration: Option<String>,

    #[arg(long = "ramp-up", alias = "ramp_up", value_name = "DURATION")]
    pub ramp_up: Option<String>,

    #[arg(long, value_name = "DURATION")]
    pub warmup: Option<String>,

    #[arg(long, value_name = "DURATION")]
    pub max_duration: Option<String>,

    #[arg(long, value_name = "RPS")]
    pub max_rps: Option<f64>,

    #[arg(long = "load-schedule", value_name = "SCHEDULE")]
    pub load_schedule: Option<String>,

    #[arg(long = "load-start", value_name = "RPS")]
    pub load_start: Option<f64>,

    #[arg(long = "load-step", value_name = "RPS_DELTA")]
    pub load_step: Option<f64>,

    #[arg(long = "load-end", value_name = "RPS")]
    pub load_end: Option<f64>,

    #[arg(long = "load-step-duration", value_name = "DURATION")]
    pub load_step_duration: Option<String>,

    #[arg(long = "load-max-duration", value_name = "DURATION")]
    pub load_max_duration: Option<String>,

    #[arg(long = "concurrency-schedule", value_name = "SCHEDULE")]
    pub concurrency_schedule: Option<String>,

    #[arg(long = "concurrency-start", value_name = "N")]
    pub concurrency_start: Option<u32>,

    #[arg(long = "concurrency-end", value_name = "N")]
    pub concurrency_end: Option<u32>,

    #[arg(long = "concurrency-step", value_name = "N")]
    pub concurrency_step: Option<u32>,

    #[arg(long = "concurrency-step-duration", value_name = "DURATION")]
    pub concurrency_step_duration: Option<String>,

    #[arg(long, value_name = "N")]
    pub connections: Option<u32>,

    #[arg(long, value_name = "DURATION")]
    pub connect_timeout: Option<String>,

    #[arg(long, value_name = "DURATION")]
    pub request_timeout: Option<String>,

    #[arg(long, value_name = "DURATION")]
    pub keepalive: Option<String>,

    #[arg(long, value_name = "N")]
    pub cpus: Option<usize>,

    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    #[arg(long, visible_alias = "bench-assert-mode", value_name = "MODE")]
    pub assert_mode: Option<String>,

    #[arg(long, visible_alias = "bench-no-assert", default_value_t = false)]
    pub no_assert: bool,

    #[arg(long, default_value_t = false)]
    pub calibrate: bool,

    #[arg(long, value_name = "RATE")]
    pub sample_rate: Option<f64>,

    #[arg(long)]
    pub cache: Option<bool>,

    #[arg(long, value_name = "N")]
    pub skip_first: Option<u32>,

    #[arg(long)]
    pub count_errors_in_latency: Option<bool>,

    #[arg(long, value_name = "MODE")]
    pub duration_stop: Option<String>,

    #[arg(long, value_name = "LIST")]
    pub latency_percentiles: Option<String>,

    #[arg(long = "progress-interval", value_name = "DURATION")]
    pub progress_interval: Option<String>,

    #[arg(
        long = "log-format",
        visible_alias = "bench-format",
        default_value = "console",
        value_name = "FORMAT"
    )]
    pub format: String,

    #[arg(
        short = 'o',
        long = "log-output",
        visible_alias = "bench-output",
        value_name = "FILE"
    )]
    pub output: Option<PathBuf>,

    #[arg(long, value_name = "TEMPLATE_FILE")]
    pub report_template: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    pub allure_output_dir: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub compact: bool,

    #[arg(long = "tags", value_name = "TAG")]
    pub tags: Vec<String>,

    #[arg(long = "skip-tags", value_name = "TAG")]
    pub skip_tags: Vec<String>,

    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    #[arg(long, default_value_t = false)]
    pub list_profiles: bool,

    #[arg(long, value_name = "FILE")]
    pub profile_file: Option<PathBuf>,

    #[arg(long, value_name = "SERVICE/METHOD")]
    pub call: Option<String>,

    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct BenchAggregateArgs {
    #[arg(required = true, value_name = "FILE")]
    pub reports: Vec<PathBuf>,

    #[arg(long, default_value = "json", value_name = "FORMAT")]
    pub format: String,

    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct BenchCompareArgs {
    #[arg(required = true, value_name = "FILE")]
    pub baseline: PathBuf,

    #[arg(required = true, value_name = "FILE")]
    pub current: PathBuf,

    #[arg(long, value_name = "PCT", default_value_t = 10.0)]
    pub max_latency_regression: f64,

    #[arg(long, value_name = "POINTS", default_value_t = 1.0)]
    pub max_error_rate_regression: f64,

    #[arg(long, value_name = "PCT", default_value_t = 5.0)]
    pub min_throughput: f64,

    #[arg(long, default_value = "console", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct IndexArgs {
    #[arg(required = true, value_name = "PATH")]
    pub sources: Vec<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub force: bool,

    #[arg(long, default_value_t = false)]
    pub stats: bool,
}

#[derive(Args, Debug, Clone)]
pub struct QueryArgs {
    #[arg(required = false, value_name = "PATH")]
    pub files: Vec<PathBuf>,

    #[arg(short = 'q', long, value_name = "EXPR")]
    pub query: Option<String>,

    #[arg(short = 's', long, default_value_t = false)]
    pub shell: bool,

    #[arg(short = 'i', long, value_name = "COLUMN")]
    pub indexed_by: Option<String>,

    #[arg(short = 'f', long, default_value = "table", value_name = "FORMAT")]
    pub format: String,

    #[arg(short = 'n', long, value_name = "N")]
    pub limit: Option<usize>,

    #[arg(short = 'o', long, value_name = "N")]
    pub offset: Option<usize>,

    #[arg(short = 'c', long, value_name = "COLS")]
    pub columns: Option<String>,

    #[arg(long, value_name = "COLUMN")]
    pub order_by: Option<String>,

    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub no_header: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    #[arg(required = false, value_name = "PATH")]
    pub path: Option<PathBuf>,

    #[arg(long, default_value = "json", value_name = "FORMAT")]
    pub format: String,

    #[arg(long, default_value_t = false)]
    pub with_range: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DocsArgs {
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    #[arg(long, short = 'o', default_value = "docs/api", value_name = "DIR")]
    pub output: PathBuf,

    #[arg(long, value_name = "PATH")]
    pub coverage: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct GraphArgs {
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct ExplainArgs {
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    #[arg(long, value_name = "REPORT_JSON")]
    pub against: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    #[arg(required = true, value_name = "FILES")]
    pub files: Vec<PathBuf>,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    #[arg(long, default_value_t = false)]
    pub bench: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    #[arg(required = false, value_name = "PATH")]
    pub test_paths: Vec<PathBuf>,

    #[arg(long = "exclude", value_name = "GLOB", help_heading = "Test Selection")]
    pub exclude: Vec<String>,

    #[arg(long = "tags", value_name = "TAG", help_heading = "Test Selection")]
    pub tags: Vec<String>,

    #[arg(
        long = "skip-tags",
        value_name = "TAG",
        help_heading = "Test Selection"
    )]
    pub skip_tags: Vec<String>,

    #[arg(
        short = 's',
        long,
        default_value = "path",
        value_name = "MODE",
        help_heading = "Test Selection"
    )]
    pub sort: String,

    #[arg(long, value_name = "PATH", help_heading = "Test Selection")]
    pub data: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FORMAT",
        requires = "data",
        help_heading = "Test Selection"
    )]
    pub data_format: Option<String>,

    #[arg(long, default_value_t = false, help_heading = "Test Selection")]
    pub only_changed: bool,

    #[arg(
        long,
        default_value = "HEAD",
        value_name = "REF",
        requires = "only_changed",
        help_heading = "Test Selection"
    )]
    pub since: String,

    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help_heading = "Execution"
    )]
    pub protocol: String,

    #[arg(
        short = 'p',
        long,
        default_value = "auto",
        value_name = "N",
        help_heading = "Execution"
    )]
    pub parallel: String,

    #[arg(
        short = 't',
        long,
        default_value_t = 30,
        value_name = "SECS",
        help_heading = "Execution"
    )]
    pub timeout: u64,

    #[arg(
        short = 'r',
        long,
        default_value_t = 0,
        value_name = "N",
        help_heading = "Execution"
    )]
    pub retry: u32,

    #[arg(
        long,
        default_value_t = 1.0,
        value_name = "SECS",
        help_heading = "Execution"
    )]
    pub retry_delay: f64,

    #[arg(long, default_value_t = false, help_heading = "Execution")]
    pub no_retry: bool,

    #[arg(long, default_value_t = false, help_heading = "Execution")]
    pub no_assert: bool,

    #[arg(short = 'w', long, default_value_t = false, help_heading = "Execution")]
    pub write: bool,

    #[arg(short = 'd', long, default_value_t = false, help_heading = "Execution")]
    pub dry_run: bool,

    #[arg(long, value_name = "FORMAT", help_heading = "Output & Reports")]
    pub log_format: Option<String>,

    #[arg(long, value_name = "PATH", help_heading = "Output & Reports")]
    pub log_output: Option<PathBuf>,

    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub stream: bool,

    #[arg(
        long,
        default_value = "auto",
        value_name = "STYLE",
        help_heading = "Output & Reports"
    )]
    pub progress: String,

    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub coverage: bool,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help_heading = "Output & Reports"
    )]
    pub coverage_format: String,

    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub capture_exchange: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReflectArgs {
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    pub symbol: Option<String>,

    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    #[arg(long, default_value_t = false)]
    pub list_methods: bool,

    #[arg(long, value_name = "SUBSTRING")]
    pub filter: Option<String>,

    #[arg(long, value_name = "SERVICE/METHOD")]
    pub describe: Option<String>,

    #[arg(long)]
    pub tls_ca: Option<String>,

    #[arg(long)]
    pub tls_cert: Option<String>,

    #[arg(long)]
    pub tls_key: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct FmtArgs {
    #[arg(required = true, value_name = "FILES")]
    pub files: Vec<PathBuf>,

    #[arg(short = 'w', long, default_value_t = false)]
    pub write: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CallArgs {
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    pub file: Option<PathBuf>,

    #[arg(
        short = 'e',
        long,
        value_name = "SERVICE/METHOD",
        conflicts_with = "file"
    )]
    pub endpoint: Option<String>,

    #[arg(short = 'd', long, value_name = "JSON", requires = "endpoint")]
    pub data: Option<String>,

    #[arg(long)]
    pub doc_index: Option<usize>,

    #[arg(short = 'H', long = "header", value_name = "NAME: VALUE")]
    pub header: Vec<String>,

    #[arg(short = 'i', long, default_value_t = false)]
    pub include: bool,

    #[arg(short = 'v', long, default_value_t = false)]
    pub verbose: bool,

    #[arg(long = "vv", default_value_t = false)]
    pub very_verbose: bool,

    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    #[arg(short = 'D', long)]
    pub dump_header: Option<PathBuf>,

    #[arg(short = 's', long, default_value_t = false)]
    pub silent: bool,

    #[arg(short = 'S', long, default_value_t = false)]
    pub show_error: bool,

    #[arg(long, default_value_t = 30, value_name = "SECS")]
    pub connect_timeout: u64,

    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    #[arg(long)]
    pub tls_ca: Option<String>,

    #[arg(long)]
    pub tls_cert: Option<String>,

    #[arg(long)]
    pub tls_key: Option<String>,

    #[arg(long, default_value_t = 60, value_name = "SECS")]
    pub max_time: u64,

    #[arg(long, default_value_t = false)]
    pub bench: bool,

    #[arg(long, requires = "bench")]
    pub concurrency: Option<u32>,

    #[arg(long, requires = "bench")]
    pub requests: Option<u64>,

    #[arg(long, requires = "bench")]
    pub duration: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct GenArgs {
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub source: GenSource,
}

#[derive(Subcommand, Debug, Clone)]
pub enum GenSource {
    Grpcurl(GenGrpcurlArgs),
}

#[derive(Args, Debug, Clone)]
#[command(trailing_var_arg = true)]
pub struct GenGrpcurlArgs {
    #[arg(short = 'e', long, default_value_t = false)]
    pub execute: bool,

    #[arg(required = true, allow_hyphen_values = true)]
    pub grpcurl_args: Vec<String>,
}

impl Cli {
    pub fn parallel_jobs(&self) -> usize {
        let parallel = match &self.command {
            Some(Commands::Run(args)) => &args.parallel,
            _ => &self.run_args.parallel,
        };

        if parallel == "auto" {
            std::thread::available_parallelism()
                .ok()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            parallel.parse().unwrap_or(1).max(1)
        }
    }

    pub fn progress_mode(&self) -> ProgressMode {
        let progress = match &self.command {
            Some(Commands::Run(args)) => &args.progress,
            _ => &self.run_args.progress,
        };

        match progress.as_str() {
            "dots" => ProgressMode::Dots,
            "bar" => ProgressMode::Dots,
            "none" => ProgressMode::None,
            "auto" => {
                if self.verbose {
                    ProgressMode::Verbose
                } else {
                    ProgressMode::Dots
                }
            }
            _ => ProgressMode::Dots,
        }
    }

    pub fn log_format_mode(&self) -> Option<LogFormat> {
        self.log_format_modes().into_iter().next()
    }

    pub fn log_format_modes(&self) -> Vec<LogFormat> {
        let log_format = match &self.command {
            Some(Commands::Run(args)) => &args.log_format,
            _ => &self.run_args.log_format,
        };

        let Some(raw) = log_format else {
            return Vec::new();
        };

        let mut seen = std::collections::HashSet::new();
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|fmt| match fmt {
                "junit" => LogFormat::JUnit,
                "json" => LogFormat::Json,
                "yaml" => LogFormat::Yaml,
                "allure" => LogFormat::Allure,
                "html" => LogFormat::Html,
                _ => LogFormat::Console,
            })
            .filter(|fmt| seen.insert(*fmt))
            .collect()
    }

    pub fn optimize_level(
        &self,
        default: crate::optimizer::OptimizeLevel,
    ) -> crate::optimizer::OptimizeLevel {
        use crate::optimizer::OptimizeLevel;
        match self.optimize.as_str() {
            "0" | "none" => OptimizeLevel::None,
            "1" | "safe" => OptimizeLevel::Safe,
            "2" | "advisory" => OptimizeLevel::Advisory,
            "3" | "aggressive" => OptimizeLevel::Aggressive,
            _ => default,
        }
    }

    pub fn get_run_args(&self) -> &RunArgs {
        match &self.command {
            Some(Commands::Run(args)) => args,
            _ => &self.run_args,
        }
    }
}

fn is_json_format(value: &str) -> bool {
    value.eq_ignore_ascii_case("json")
}

pub trait HasFormat {
    fn format(&self) -> &str;

    fn is_json(&self) -> bool {
        is_json_format(self.format())
    }
}

impl HasFormat for ListArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

impl HasFormat for InspectArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

impl HasFormat for ExplainArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

impl HasFormat for GrpcurlArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

impl HasFormat for CheckArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

impl HasFormat for BenchArgs {
    fn format(&self) -> &str {
        &self.format
    }
}

#[derive(Args, Debug, Clone)]
pub struct PlayArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value = "4755")]
    pub port: u16,

    #[arg(long, default_value = ".")]
    pub dir: std::path::PathBuf,

    #[arg(long, default_value_t = false)]
    pub open: bool,

    #[arg(long, default_value_t = false)]
    pub init: bool,
}

impl RunArgs {
    #[must_use]
    pub fn is_json_coverage(&self) -> bool {
        is_json_format(&self.coverage_format)
    }

    #[must_use]
    pub fn is_html_coverage(&self) -> bool {
        self.coverage_format.eq_ignore_ascii_case("html")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parallel_jobs_clamps_zero_to_one() {
        let cli = Cli::parse_from(["grpctestify", "run", "-p", "0", "test.gctf"]);
        assert_eq!(cli.parallel_jobs(), 1);
    }

    #[test]
    fn parallel_jobs_invalid_defaults_to_one() {
        let cli = Cli::parse_from(["grpctestify", "run", "-p", "bogus", "test.gctf"]);
        assert_eq!(cli.parallel_jobs(), 1);
    }

    #[test]
    fn parallel_jobs_auto_is_at_least_one() {
        let cli = Cli::parse_from(["grpctestify", "run", "test.gctf"]);
        assert!(cli.parallel_jobs() >= 1);
    }

    #[test]
    fn log_format_modes_single_format_unchanged() {
        let cli = Cli::parse_from(["grpctestify", "run", "--log-format", "junit", "t.gctf"]);
        assert_eq!(cli.log_format_modes(), vec![LogFormat::JUnit]);
        assert_eq!(cli.log_format_mode(), Some(LogFormat::JUnit));
    }

    #[test]
    fn log_format_modes_comma_separated() {
        let cli = Cli::parse_from(["grpctestify", "run", "--log-format", "junit,html", "t.gctf"]);
        assert_eq!(
            cli.log_format_modes(),
            vec![LogFormat::JUnit, LogFormat::Html]
        );
    }

    #[test]
    fn log_format_modes_dedups_and_trims() {
        let cli = Cli::parse_from([
            "grpctestify",
            "run",
            "--log-format",
            " junit , html,junit ",
            "t.gctf",
        ]);
        assert_eq!(
            cli.log_format_modes(),
            vec![LogFormat::JUnit, LogFormat::Html]
        );
    }

    #[test]
    fn log_format_modes_empty_when_unset() {
        let cli = Cli::parse_from(["grpctestify", "run", "t.gctf"]);
        assert!(cli.log_format_modes().is_empty());
        assert_eq!(cli.log_format_mode(), None);
    }

    #[test]
    fn capture_exchange_flag_defaults_false_and_parses() {
        let cli = Cli::parse_from(["grpctestify", "run", "t.gctf"]);
        assert!(!cli.get_run_args().capture_exchange);

        let cli = Cli::parse_from(["grpctestify", "run", "--capture-exchange", "t.gctf"]);
        assert!(cli.get_run_args().capture_exchange);
    }

    #[test]
    fn parse_call_defaults() {
        let cli = Cli::parse_from(["grpctestify", "call", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };

        assert_eq!(call.file, Some(PathBuf::from("test.gctf")));
        assert_eq!(call.doc_index, None);
        assert_eq!(call.endpoint, None);
        assert_eq!(call.data, None);
        assert!(!call.include);
        assert!(!call.verbose);
        assert!(!call.very_verbose);
        assert!(!call.silent);
        assert!(!call.show_error);
        assert_eq!(call.connect_timeout, 30);
        assert_eq!(call.max_time, 60);
    }

    #[test]
    fn parse_call_address() {
        let cli = Cli::parse_from(["grpctestify", "call", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert_eq!(call.address, None);

        let cli = Cli::parse_from([
            "grpctestify",
            "call",
            "--address",
            "staging:4770",
            "-e",
            "svc.Method/Call",
        ]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert_eq!(call.address.as_deref(), Some("staging:4770"));
    }

    #[test]
    fn parse_bench_address() {
        let cli = Cli::parse_from([
            "grpctestify",
            "bench",
            "--address",
            "staging:4770",
            "t.gctf",
        ]);
        let Some(Commands::Bench(bench)) = cli.command else {
            panic!("expected bench command");
        };
        assert_eq!(bench.address.as_deref(), Some("staging:4770"));

        assert!(
            Cli::try_parse_from([
                "grpctestify",
                "bench",
                "--address",
                "staging:4770",
                "--calibrate",
                "t.gctf",
            ])
            .is_err()
        );
    }

    #[test]
    fn parse_call_inline_endpoint() {
        let cli = Cli::parse_from([
            "grpctestify",
            "call",
            "-e",
            "svc.Method/Call",
            "-d",
            r#"{"name":"test"}"#,
        ]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert_eq!(call.endpoint.as_deref(), Some("svc.Method/Call"));
        assert_eq!(call.data.as_deref(), Some(r#"{"name":"test"}"#));
        assert_eq!(call.file, None);
    }

    #[test]
    fn parse_call_verbose_flags() {
        let cli = Cli::parse_from(["grpctestify", "call", "-v", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!()
        };
        assert!(call.verbose);
        assert!(!call.very_verbose);

        let cli = Cli::parse_from(["grpctestify", "call", "--vv", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!()
        };
        assert!(!call.verbose);
        assert!(call.very_verbose);
    }

    #[test]
    fn parse_call_include_and_dump_header() {
        let cli = Cli::parse_from(["grpctestify", "call", "-i", "-D", "/tmp/h.txt", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!()
        };
        assert!(call.include);
        assert_eq!(call.dump_header, Some(PathBuf::from("/tmp/h.txt")));
    }

    #[test]
    fn parse_call_silent_and_show_error() {
        let cli = Cli::parse_from(["grpctestify", "call", "-s", "-S", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!()
        };
        assert!(call.silent);
        assert!(call.show_error);
    }

    #[test]
    fn parse_gen_with_output_before_source() {
        let cli = Cli::parse_from([
            "grpctestify",
            "gen",
            "-o",
            "out.gctf",
            "grpcurl",
            "-plaintext",
            "localhost:4770",
            "svc.Method/Call",
        ]);

        let Some(Commands::Gen(gen_args)) = cli.command else {
            panic!("expected gen command");
        };
        assert_eq!(gen_args.output, Some(PathBuf::from("out.gctf")));

        let GenSource::Grpcurl(grpcurl) = gen_args.source;
        assert_eq!(
            grpcurl.grpcurl_args,
            vec![
                "-plaintext".to_string(),
                "localhost:4770".to_string(),
                "svc.Method/Call".to_string()
            ]
        );
    }

    #[test]
    fn parse_gen_grpcurl_preserves_hyphen_args() {
        let cli = Cli::parse_from([
            "grpctestify",
            "gen",
            "grpcurl",
            "-H",
            "x-api-key: abc",
            "-d",
            "{}",
            "localhost:4770",
            "svc.Method/Call",
        ]);

        let Some(Commands::Gen(gen_args)) = cli.command else {
            panic!("expected gen command");
        };

        let GenSource::Grpcurl(grpcurl) = gen_args.source;
        assert_eq!(grpcurl.grpcurl_args[0], "-H");
        assert_eq!(grpcurl.grpcurl_args[2], "-d");
        assert_eq!(grpcurl.grpcurl_args[3], "{}");
        assert_eq!(grpcurl.grpcurl_args[4], "localhost:4770");
    }

    #[test]
    fn parse_bench_extended_options() {
        let cli = Cli::parse_from([
            "grpctestify",
            "bench",
            "tests/",
            "-c",
            "8",
            "-n",
            "1000",
            "--max-duration",
            "30s",
            "--connections",
            "4",
            "--connect-timeout",
            "2s",
            "--keepalive",
            "10s",
            "--cpus",
            "2",
            "--name",
            "smoke-bench",
        ]);

        let Some(Commands::Bench(args)) = cli.command else {
            panic!("expected bench command");
        };

        assert_eq!(args.test_paths, vec![PathBuf::from("tests/")]);
        assert_eq!(args.concurrency, Some(8));
        assert_eq!(args.requests, Some(1000));
        assert_eq!(args.max_duration.as_deref(), Some("30s"));
        assert_eq!(args.connections, Some(4));
        assert_eq!(args.connect_timeout.as_deref(), Some("2s"));
        assert_eq!(args.keepalive.as_deref(), Some("10s"));
        assert_eq!(args.cpus, Some(2));
        assert_eq!(args.name.as_deref(), Some("smoke-bench"));
    }

    #[test]
    fn parse_bench_run_style_option_names() {
        let cli = Cli::parse_from([
            "grpctestify",
            "bench",
            "tests/",
            "--no-assert",
            "--assert-mode",
            "sampled",
            "--log-format",
            "json",
            "--log-output",
            "bench.json",
            "--latency-percentiles",
            "p50,p90,p99",
            "--duration-stop",
            "wait",
            "--progress-interval",
            "3s",
            "--ramp-up",
            "3s",
            "--warmup",
            "1s",
        ]);

        let Some(Commands::Bench(args)) = cli.command else {
            panic!("expected bench command");
        };

        assert!(args.no_assert);
        assert_eq!(args.assert_mode.as_deref(), Some("sampled"));
        assert_eq!(args.format, "json");
        assert_eq!(args.output, Some(PathBuf::from("bench.json")));
        assert_eq!(args.latency_percentiles.as_deref(), Some("p50,p90,p99"));
        assert_eq!(args.duration_stop.as_deref(), Some("wait"));
        assert_eq!(args.progress_interval.as_deref(), Some("3s"));
        assert_eq!(args.ramp_up.as_deref(), Some("3s"));
        assert_eq!(args.warmup.as_deref(), Some("1s"));
    }

    #[test]
    fn parse_bench_load_schedule_options() {
        let cli = Cli::parse_from([
            "grpctestify",
            "bench",
            "tests/",
            "-c",
            "10",
            "-n",
            "10000",
            "--load-schedule",
            "step",
            "--load-start",
            "50",
            "--load-end",
            "150",
            "--load-step",
            "10",
            "--load-step-duration",
            "5s",
            "--load-max-duration",
            "40s",
        ]);

        let Some(Commands::Bench(args)) = cli.command else {
            panic!("expected bench command");
        };

        assert_eq!(args.concurrency, Some(10));
        assert_eq!(args.requests, Some(10000));
        assert_eq!(args.load_schedule.as_deref(), Some("step"));
        assert_eq!(args.load_start, Some(50.0));
        assert_eq!(args.load_end, Some(150.0));
        assert_eq!(args.load_step, Some(10.0));
        assert_eq!(args.load_step_duration.as_deref(), Some("5s"));
        assert_eq!(args.load_max_duration.as_deref(), Some("40s"));
    }

    #[test]
    fn parse_index_command() {
        let cli = Cli::parse_from([
            "grpctestify",
            "index",
            "tests/bench/user_lookup.gctf",
            "--force",
        ]);

        let Some(Commands::Index(args)) = cli.command else {
            panic!("expected index command");
        };
        assert_eq!(
            args.sources,
            vec![PathBuf::from("tests/bench/user_lookup.gctf")]
        );
        assert!(args.force);
    }
}
