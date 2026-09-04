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
    about = "Native, zero-dependency gRPC and HTTP testing with .gctf, .httf and .apif files",
    long_about = "Native, zero-dependency gRPC and HTTP testing with .gctf, .httf and .apif files.\n\n\
        Run declarative tests against gRPC / gRPC-Web / Connect and plain HTTP endpoints,\n\
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

    #[arg(
        short = 'v',
        long,
        global = true,
        default_value_t = false,
        help = "Enable verbose debug output"
    )]
    pub verbose: bool,

    #[arg(
        long = "optimize",
        short = 'O',
        value_name = "LEVEL",
        global = true,
        default_value_t = String::new(),
        help = "Optimizer level (0=none, layout, 1=safe, 2=advisory, 3=aggressive)"
    )]
    pub optimize: String,

    #[arg(
        long,
        value_name = "SHELL_TYPE",
        value_parser = ["bash", "zsh", "fish", "elvish", "powershell"],
        help = "Install shell completion (bash, zsh, fish, elvish, powershell)"
    )]
    pub completion: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Run .gctf / .httf / .apif tests (default)")]
    Run(Box<RunArgs>),
    #[command(about = "Validate test file syntax & semantics")]
    Check(CheckArgs),
    #[command(about = "Format test files in place")]
    Fmt(FmtArgs),

    #[command(about = "Generate a .gctf from proto/descriptor/reflection")]
    Scaffold(ScaffoldArgs),
    #[command(about = "Generate a .gctf from a captured invocation")]
    Gen(GenArgs),

    #[command(about = "List services & methods via server reflection")]
    Reflect(ReflectArgs),
    #[command(about = "Show a test's structure & metadata")]
    Inspect(InspectArgs),
    #[command(about = "Explain a test's execution flow")]
    Explain(ExplainArgs),
    #[command(about = "Print the equivalent grpcurl command")]
    Grpcurl(GrpcurlArgs),
    #[command(about = "List discovered test files")]
    List(ListArgs),
    #[command(about = "Generate Markdown API docs from test files")]
    Docs(DocsArgs),
    #[command(about = "Visualize fixture setup/teardown topology and multi-document chains")]
    Graph(GraphArgs),

    #[command(about = "Call a gRPC method or HTTP endpoint without assertions")]
    Call(CallArgs),
    #[command(about = "Check a server's gRPC health")]
    Health(HealthArgs),

    #[command(about = "Load-test endpoints")]
    Bench(BenchArgs),
    #[command(about = "Compare two bench reports & gate regressions")]
    BenchCompare(BenchCompareArgs),
    #[command(about = "Fold many bench reports into one matrix document")]
    BenchAggregate(BenchAggregateArgs),

    #[command(about = "Build & manage data-source indexes")]
    Index(IndexArgs),
    #[command(about = "Query a data source (CSV/TSV/NDJSON)")]
    Query(QueryArgs),

    #[command(about = "Launch the web playground")]
    Play(PlayArgs),
    #[command(about = "Run the .gctf language server (LSP)")]
    Lsp(LspArgs),

    #[command(about = "Install/manage .rhai plugins from a git host")]
    Plugins(PluginsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub action: PluginsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PluginsAction {
    #[command(about = "Install a plugin from host/owner/repo[/subpath][@spec]")]
    Install(PluginsInstallArgs),
    #[command(about = "List installed plugins")]
    List(PluginsListArgs),
    #[command(about = "Remove an installed plugin")]
    Remove(PluginsRemoveArgs),
    #[command(about = "Re-resolve and re-fetch installed plugins")]
    Update(PluginsUpdateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsInstallArgs {
    #[arg(
        help = "host/owner/repo[/subpath][@spec] — spec is a tag/branch (exact) or a semver range like ^1.2.0",
        long_help = "host/owner/repo[/subpath][@spec] — spec is a tag/branch (exact) or a semver range like ^1.2.0 (omit for the highest semver tag, or HEAD if the repo has none)"
    )]
    pub source: String,

    #[arg(
        short = 'g',
        long,
        default_value_t = false,
        help = "Install to $HOME/.grpctestify instead of the project-local ./.grpctestify"
    )]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsListArgs {
    #[arg(
        short = 'g',
        long,
        default_value_t = false,
        help = "List the user-global tier ($HOME/.grpctestify) instead of the project-local one"
    )]
    pub global: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "List both the project-local and user-global tiers"
    )]
    pub all: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsRemoveArgs {
    #[arg(help = "host/owner/repo, as shown by `plugins list`")]
    pub name: String,

    #[arg(
        short = 'g',
        long,
        default_value_t = false,
        help = "Remove from $HOME/.grpctestify instead of the project-local ./.grpctestify"
    )]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsUpdateArgs {
    #[arg(help = "host/owner/repo to update (omit to update every installed plugin in the tier)")]
    pub name: Option<String>,

    #[arg(
        short = 'g',
        long,
        default_value_t = false,
        help = "Update the user-global tier ($HOME/.grpctestify) instead of the project-local one"
    )]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ScaffoldArgs {
    #[arg(
        long,
        value_name = "SERVICE/METHOD",
        required = true,
        help = "Fully-qualified method to scaffold (package.Service/Method)"
    )]
    pub endpoint: String,

    #[arg(
        long,
        value_name = "FILE_OR_DIR",
        help = "Proto file or directory to compile (pure-Rust protox, no protoc)"
    )]
    pub proto: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        help = "Pre-compiled FileDescriptorSet (.protoset/.pb)"
    )]
    pub descriptor: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help = "Load descriptors from server reflection at --address"
    )]
    pub reflect: bool,

    #[arg(long, value_name = "ADDRESS", help = "Server address (host:port)")]
    pub address: Option<String>,

    #[arg(
        short = 'o',
        long,
        value_name = "FILE",
        help = "Output file (stdout if omitted)"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help = "Overwrite the output file if it already exists"
    )]
    pub force: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Use TLS with certificate verification"
    )]
    pub tls: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip TLS certificate verification"
    )]
    pub insecure: bool,

    #[arg(long, default_value_t = false, help = "Plaintext connection (no TLS)")]
    pub plaintext: bool,

    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help = "Wire protocol: grpc, grpc-web, connectrpc"
    )]
    pub protocol: String,
}

#[derive(Args, Debug, Clone)]
pub struct HealthArgs {
    #[arg(
        required = true,
        value_name = "ADDRESS",
        help = "Server address (host:port)"
    )]
    pub address: String,

    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help = "Wire protocol: grpc, grpc-web, connectrpc"
    )]
    pub protocol: String,

    #[arg(
        long,
        value_name = "NAME",
        help = "Service name to check (repeatable; default: none — checks overall server health)"
    )]
    pub service: Vec<String>,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,

    #[arg(
        long,
        default_value_t = false,
        help = "Use TLS with certificate verification"
    )]
    pub tls: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip TLS certificate verification"
    )]
    pub insecure: bool,

    #[arg(
        long,
        default_value_t = 10,
        value_name = "SECS",
        help = "Timeout in seconds (per health-check RPC)"
    )]
    pub timeout: u64,

    #[arg(
        long,
        default_value_t = false,
        help = "Poll until healthy instead of checking once (for CI readiness gating)"
    )]
    pub watch: bool,

    #[arg(
        long,
        default_value_t = 1.0,
        value_name = "SECS",
        help = "Poll interval in seconds for --watch"
    )]
    pub interval: f64,

    #[arg(
        long,
        default_value_t = 60.0,
        value_name = "SECS",
        help = "Give up waiting after this many seconds in --watch mode"
    )]
    pub watch_timeout: f64,
}

#[derive(Args, Debug, Clone)]
pub struct GrpcurlArgs {
    #[arg(required = true, value_name = "FILE", help = "File to convert")]
    pub file: PathBuf,

    #[arg(
        long,
        value_name = "N",
        help = "Document index for multi-document files (1-based)"
    )]
    pub doc_index: Option<usize>,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    #[arg(
        long,
        default_value_t = true,
        help = "Use stdio for communication (default)"
    )]
    pub stdio: bool,
}

#[derive(Args, Debug, Clone)]
pub struct BenchArgs {
    #[arg(
        long,
        value_name = "ADDRESS",
        conflicts_with = "calibrate",
        help = "Server address (host:port) to load, overriding the files' ADDRESS and $GRPCTESTIFY_ADDRESS"
    )]
    pub address: Option<String>,

    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help = "Wire protocol: grpc, grpc-web, connectrpc"
    )]
    pub protocol: String,

    #[arg(
        required = true,
        value_name = "PATH",
        help = "Test files or directories to benchmark"
    )]
    pub test_paths: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "PROFILE",
        help = "Benchmark profile (functional, load, stress, spike, soak)"
    )]
    pub profile: Option<String>,

    #[arg(
        long,
        value_name = "MODE",
        help = "Benchmark mode (fixed, stepping, adaptive)"
    )]
    pub mode: Option<String>,

    #[arg(
        short = 'c',
        long,
        value_name = "N",
        help = "Number of concurrent workers"
    )]
    pub concurrency: Option<u32>,

    #[arg(
        short = 'n',
        long,
        value_name = "N",
        help = "Total number of requests to send"
    )]
    pub requests: Option<u64>,

    #[arg(
        short = 'd',
        long,
        value_name = "DURATION",
        help = "Duration of benchmark (e.g., 30s, 5m)"
    )]
    pub duration: Option<String>,

    #[arg(
        long = "ramp-up",
        alias = "ramp_up",
        value_name = "DURATION",
        help = "Ramp-up duration before steady-state load (e.g., 10s)"
    )]
    pub ramp_up: Option<String>,

    #[arg(
        long,
        value_name = "DURATION",
        help = "Warmup period excluded from final metrics (e.g., 5s)"
    )]
    pub warmup: Option<String>,

    #[arg(
        long,
        value_name = "DURATION",
        help = "Maximum runtime with request-count mode (e.g., 30s, 5m)"
    )]
    pub max_duration: Option<String>,

    #[arg(
        long,
        value_name = "RPS",
        help = "Maximum requests per second (rate limit)"
    )]
    pub max_rps: Option<f64>,

    #[arg(
        long = "load-schedule",
        value_name = "SCHEDULE",
        help = "Load schedule strategy (const, step, line)"
    )]
    pub load_schedule: Option<String>,

    #[arg(
        long = "load-start",
        value_name = "RPS",
        help = "Starting RPS for step/line load schedules"
    )]
    pub load_start: Option<f64>,

    #[arg(
        long = "load-step",
        value_name = "RPS_DELTA",
        help = "Step/slope RPS delta for step/line schedules"
    )]
    pub load_step: Option<f64>,

    #[arg(
        long = "load-end",
        value_name = "RPS",
        help = "Optional ending RPS for step/line schedules"
    )]
    pub load_end: Option<f64>,

    #[arg(
        long = "load-step-duration",
        value_name = "DURATION",
        help = "Duration of each step for step schedule"
    )]
    pub load_step_duration: Option<String>,

    #[arg(
        long = "load-max-duration",
        value_name = "DURATION",
        help = "Maximum duration of load adjustments"
    )]
    pub load_max_duration: Option<String>,

    #[arg(
        long = "concurrency-schedule",
        value_name = "SCHEDULE",
        help = "Concurrency schedule: const, step, line — a second axis alongside the --load-* (RPS) schedule",
        long_help = "Concurrency schedule: const, step, line — a second axis alongside the --load-* (RPS) schedule. Each level is measured in full and reported separately."
    )]
    pub concurrency_schedule: Option<String>,

    #[arg(
        long = "concurrency-start",
        value_name = "N",
        help = "First concurrency level for a step/line schedule"
    )]
    pub concurrency_start: Option<u32>,

    #[arg(
        long = "concurrency-end",
        value_name = "N",
        help = "Last concurrency level for a step/line schedule (inclusive)"
    )]
    pub concurrency_end: Option<u32>,

    #[arg(
        long = "concurrency-step",
        value_name = "N",
        help = "Worker delta between concurrency levels (default 1 for line, 0 = auto)"
    )]
    pub concurrency_step: Option<u32>,

    #[arg(
        long = "concurrency-step-duration",
        value_name = "DURATION",
        help = "Per-level run duration, overriding the run's own stop condition"
    )]
    pub concurrency_step_duration: Option<String>,

    #[arg(
        long,
        value_name = "N",
        help = "Number of gRPC connections to use (<= concurrency)"
    )]
    pub connections: Option<u32>,

    #[arg(long, value_name = "DURATION", help = "Connection timeout (e.g., 10s)")]
    pub connect_timeout: Option<String>,

    #[arg(
        long,
        value_name = "DURATION",
        help = "Per-request timeout (e.g., 120s); defaults to the benchmark duration"
    )]
    pub request_timeout: Option<String>,

    #[arg(long, value_name = "DURATION", help = "Keepalive interval (e.g., 30s)")]
    pub keepalive: Option<String>,

    #[arg(
        long,
        value_name = "N",
        help = "Tokio worker threads for the run (default: min(cores, 4); also honours TOKIO_WORKER_THREADS)",
        long_help = "Tokio worker threads for the run. Defaults to min(cores, 4) for `bench`; more threads cost CPU and instructions per request without adding throughput. Also honours TOKIO_WORKER_THREADS."
    )]
    pub cpus: Option<usize>,

    #[arg(long, value_name = "NAME", help = "User-defined benchmark run name")]
    pub name: Option<String>,

    #[arg(
        long,
        visible_alias = "bench-assert-mode",
        value_name = "MODE",
        help = "Assertion mode (fail_fast, collect_all, skip)"
    )]
    pub assert_mode: Option<String>,

    #[arg(
        long,
        visible_alias = "bench-no-assert",
        default_value_t = false,
        help = "Disable ASSERTS evaluation to measure transport baseline"
    )]
    pub no_assert: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Measure this client's own floor: run against a built-in no-op target instead of the configured address"
    )]
    pub calibrate: bool,

    #[arg(
        long,
        value_name = "RATE",
        help = "Sample rate for detailed logging (0.0-1.0)"
    )]
    pub sample_rate: Option<f64>,

    #[arg(long, value_name = "BOOL", help = "Enable reflection/proto caching")]
    pub cache: Option<bool>,

    #[arg(
        long,
        value_name = "N",
        help = "Skip first N requests in latency metrics"
    )]
    pub skip_first: Option<u32>,

    #[arg(
        long,
        value_name = "BOOL",
        help = "Include errors in latency calculation"
    )]
    pub count_errors_in_latency: Option<bool>,

    #[arg(
        long,
        value_name = "MODE",
        help = "In-flight handling when duration limit is reached (close, wait, ignore)"
    )]
    pub duration_stop: Option<String>,

    #[arg(
        long,
        value_name = "LIST",
        help = "Latency percentiles to report (comma-separated, e.g. p50,p90,p95,p99)"
    )]
    pub latency_percentiles: Option<String>,

    #[arg(
        long = "progress-interval",
        value_name = "DURATION",
        help = "Progress heartbeat interval (e.g. 5s)"
    )]
    pub progress_interval: Option<String>,

    #[arg(
        long = "log-format",
        visible_alias = "bench-format",
        default_value = "console",
        value_name = "FORMAT",
        help = "Report format: console, json, csv, ndjson, prometheus"
    )]
    pub format: String,

    #[arg(
        short = 'o',
        long = "log-output",
        visible_alias = "bench-output",
        value_name = "FILE",
        help = "Output file for the report"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        value_name = "TEMPLATE_FILE",
        help = "Custom MiniJinja template file for benchmark report"
    )]
    pub report_template: Option<PathBuf>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Allure output directory for benchmark attachments"
    )]
    pub allure_output_dir: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help = "Compact console output (omit histogram)"
    )]
    pub compact: bool,

    #[arg(
        long = "tags",
        value_name = "TAG",
        help = "Only run tests carrying ALL of these tags (repeatable)"
    )]
    pub tags: Vec<String>,

    #[arg(
        long = "skip-tags",
        value_name = "TAG",
        help = "Skip tests carrying ANY of these tags (repeatable)"
    )]
    pub skip_tags: Vec<String>,

    #[arg(
        long,
        value_name = "GLOB",
        help = "Exclude paths matching this glob (repeatable)"
    )]
    pub exclude: Vec<String>,

    #[arg(
        long,
        default_value_t = false,
        help = "List available benchmark profiles and exit"
    )]
    pub list_profiles: bool,

    #[arg(long, value_name = "FILE", help = "Path to custom profile YAML file")]
    pub profile_file: Option<PathBuf>,

    #[arg(
        long,
        value_name = "SERVICE/METHOD",
        help = "Direct gRPC method call (service/method) — no test file needed"
    )]
    pub call: Option<String>,

    #[arg(
        long,
        value_name = "JSON",
        help = "Inline JSON request body (used with --call)"
    )]
    pub data: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct BenchAggregateArgs {
    #[arg(
        required = true,
        value_name = "FILE",
        help = "Bench reports to fold together (JSON produced by `bench --log-format json`)"
    )]
    pub reports: Vec<PathBuf>,

    #[arg(
        long,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format: json (one matrix document) or csv (one row per level)"
    )]
    pub format: String,

    #[arg(
        short = 'o',
        long,
        value_name = "FILE",
        help = "Write to this path instead of stdout"
    )]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct BenchCompareArgs {
    #[arg(
        required = true,
        value_name = "FILE",
        help = "Baseline bench report (JSON produced by `bench --log-format json`)"
    )]
    pub baseline: PathBuf,

    #[arg(
        required = true,
        value_name = "FILE",
        help = "Current bench report to compare against the baseline"
    )]
    pub current: PathBuf,

    #[arg(
        long,
        value_name = "PCT",
        default_value_t = 10.0,
        help = "Max tolerated latency rise (percent) for mean and each percentile"
    )]
    pub max_latency_regression: f64,

    #[arg(
        long,
        value_name = "POINTS",
        default_value_t = 1.0,
        help = "Max tolerated error-rate rise, in percentage points"
    )]
    pub max_error_rate_regression: f64,

    #[arg(
        long,
        value_name = "PCT",
        default_value_t = 5.0,
        help = "Max tolerated throughput (rps) drop (percent) before failing"
    )]
    pub min_throughput: f64,

    #[arg(
        long,
        default_value = "console",
        value_name = "FORMAT",
        help = "Report format: console, json"
    )]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct IndexArgs {
    #[arg(
        required = true,
        value_name = "PATH",
        help = "Test file(s) or directory with BENCH.sources definitions"
    )]
    pub sources: Vec<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help = "Force rebuild of all required indexes"
    )]
    pub force: bool,

    #[arg(long, default_value_t = false, help = "Show index file statistics")]
    pub stats: bool,
}

#[derive(Args, Debug, Clone)]
pub struct QueryArgs {
    #[arg(
        required = false,
        value_name = "PATH",
        help = "Files or directories to query (default: interactive shell)"
    )]
    pub files: Vec<PathBuf>,

    #[arg(
        short = 'q',
        long,
        value_name = "EXPR",
        help = "Query expression to execute"
    )]
    pub query: Option<String>,

    #[arg(
        short = 's',
        long,
        default_value_t = false,
        help = "Run in interactive shell mode"
    )]
    pub shell: bool,

    #[arg(
        short = 'i',
        long,
        value_name = "COLUMN",
        help = "Index column for direct file mode"
    )]
    pub indexed_by: Option<String>,

    #[arg(
        short = 'f',
        long,
        default_value = "table",
        value_name = "FORMAT",
        help = "Output format: json, csv, table, line, tsv"
    )]
    pub format: String,

    #[arg(
        short = 'n',
        long,
        value_name = "N",
        help = "Maximum number of rows to return"
    )]
    pub limit: Option<usize>,

    #[arg(short = 'o', long, value_name = "N", help = "Skip N rows")]
    pub offset: Option<usize>,

    #[arg(
        short = 'c',
        long,
        value_name = "COLS",
        help = "Output columns (comma-separated)"
    )]
    pub columns: Option<String>,

    #[arg(
        long,
        value_name = "COLUMN",
        help = "Sort by column (prefix with - for DESC)"
    )]
    pub order_by: Option<String>,

    #[arg(long, value_name = "FILE", help = "Output file (stdout if omitted)")]
    pub output: Option<PathBuf>,

    #[arg(long, default_value_t = false, help = "Skip header row in output")]
    pub no_header: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    #[arg(
        required = false,
        value_name = "PATH",
        help = "Test file or directory to list"
    )]
    pub path: Option<PathBuf>,

    #[arg(
        long,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,

    #[arg(long, default_value_t = false, help = "Include test range information")]
    pub with_range: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DocsArgs {
    #[arg(
        value_name = "PATH",
        help = "Test files or directories to document (defaults to the current directory)"
    )]
    pub paths: Vec<PathBuf>,

    #[arg(
        long,
        short = 'o',
        default_value = "docs/api",
        value_name = "DIR",
        help = "Directory to write the generated Markdown into"
    )]
    pub output: PathBuf,

    #[arg(
        long,
        value_name = "PATH",
        help = "Embed method/field coverage from a prior `run --coverage --coverage-format json` report"
    )]
    pub coverage: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct GraphArgs {
    #[arg(
        value_name = "PATH",
        help = "Test files or directories to visualize (defaults to the current directory)"
    )]
    pub paths: Vec<PathBuf>,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, mermaid"
    )]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    #[arg(required = true, value_name = "FILE", help = "File to inspect")]
    pub file: PathBuf,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct ExplainArgs {
    #[arg(required = true, value_name = "FILE", help = "File to explain")]
    pub file: PathBuf,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,

    #[arg(
        long,
        value_name = "REPORT_JSON",
        help = "Post-hoc mode: JSON report from a prior `run --log-format json` to correlate against this file's plan",
        long_help = "Post-hoc mode: JSON report from a prior `run --log-format json` to correlate against this file's plan (actual per-assertion pass/fail + timing, instead of just the static/optimized plan)."
    )]
    pub against: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    #[arg(required = true, value_name = "FILES", help = "Files to validate")]
    pub files: Vec<PathBuf>,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,

    #[arg(
        long,
        default_value_t = false,
        help = "Validate BENCH section configuration"
    )]
    pub bench: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    #[arg(
        required = false,
        value_name = "PATH",
        help = "Test files or directories to run (defaults to the current directory)"
    )]
    pub test_paths: Vec<PathBuf>,

    #[arg(
        long = "exclude",
        value_name = "GLOB",
        help_heading = "Test Selection",
        help = "Exclude paths matching this glob (repeatable)"
    )]
    pub exclude: Vec<String>,

    #[arg(
        long = "tags",
        value_name = "TAG",
        help_heading = "Test Selection",
        help = "Only run tests carrying ALL of these tags (repeatable)"
    )]
    pub tags: Vec<String>,

    #[arg(
        long = "skip-tags",
        value_name = "TAG",
        help_heading = "Test Selection",
        help = "Skip tests carrying ANY of these tags (repeatable)"
    )]
    pub skip_tags: Vec<String>,

    #[arg(
        short = 's',
        long,
        default_value = "path",
        value_name = "MODE",
        help_heading = "Test Selection",
        help = "Order tests before running: path, size, name"
    )]
    pub sort: String,

    #[arg(
        long,
        value_name = "PATH",
        help_heading = "Test Selection",
        help = "Data source (CSV/TSV/NDJSON) driving each test file as a template — one case per row ({{source.column}} substitution)"
    )]
    pub data: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FORMAT",
        requires = "data",
        help_heading = "Test Selection",
        help = "Override the --data format (csv, tsv, ndjson); inferred from the extension otherwise"
    )]
    pub data_format: Option<String>,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Test Selection",
        help = "Only run tests whose file content differs from --since (git, no shell-out)"
    )]
    pub only_changed: bool,

    #[arg(
        long,
        default_value = "HEAD",
        value_name = "REF",
        requires = "only_changed",
        help_heading = "Test Selection",
        help = "Git ref --only-changed compares against (default: HEAD, i.e. uncommitted changes)"
    )]
    pub since: String,

    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help_heading = "Execution",
        help = "Wire protocol: grpc, grpc-web, connectrpc"
    )]
    pub protocol: String,

    #[arg(
        short = 'p',
        long,
        default_value = "auto",
        value_name = "N",
        help_heading = "Execution",
        help = "Worker count for parallel runs, or `auto`"
    )]
    pub parallel: String,

    #[arg(
        short = 't',
        long,
        default_value_t = 30,
        value_name = "SECS",
        help_heading = "Execution",
        help = "Per-test timeout in seconds"
    )]
    pub timeout: u64,

    #[arg(
        short = 'r',
        long,
        default_value_t = 0,
        value_name = "N",
        help_heading = "Execution",
        help = "Retry failed network calls up to N times"
    )]
    pub retry: u32,

    #[arg(
        long,
        default_value_t = 1.0,
        value_name = "SECS",
        help_heading = "Execution",
        help = "Initial delay between retries in seconds"
    )]
    pub retry_delay: f64,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Execution",
        help = "Disable retries entirely"
    )]
    pub no_retry: bool,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Execution",
        help = "Skip assertions and print raw server responses"
    )]
    pub no_assert: bool,

    #[arg(
        short = 'w',
        long,
        default_value_t = false,
        help_heading = "Execution",
        help = "Snapshot mode: write actual server responses back into the test files"
    )]
    pub write: bool,

    #[arg(
        short = 'd',
        long,
        default_value_t = false,
        help_heading = "Execution",
        help = "Show what would run without executing anything"
    )]
    pub dry_run: bool,

    #[arg(
        long,
        value_name = "FORMAT",
        help_heading = "Output & Reports",
        help = "Report format: junit, json, yaml, allure, html (comma-separated for several, e.g. junit,html)"
    )]
    pub log_format: Option<String>,

    #[arg(
        long,
        value_name = "PATH",
        help_heading = "Output & Reports",
        help = "Destination for --log-format: exact file (or directory, for allure) for one format; a directory holding one file per format for several"
    )]
    pub log_output: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Output & Reports",
        help = "Emit streaming NDJSON events (for IDE/CI integration)"
    )]
    pub stream: bool,

    #[arg(
        long,
        default_value = "auto",
        value_name = "STYLE",
        help_heading = "Output & Reports",
        help = "Progress style: auto, dots, verbose, none"
    )]
    pub progress: String,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Output & Reports",
        help = "Report proto API coverage after the run"
    )]
    pub coverage: bool,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help_heading = "Output & Reports",
        help = "Coverage format: text, json, html"
    )]
    pub coverage_format: String,

    #[arg(
        long,
        default_value_t = false,
        help_heading = "Output & Reports",
        help = "Force-capture the request/response exchange even when the active reporter wouldn't otherwise need it",
        long_help = "Force-capture the request/response exchange even when the active reporter wouldn't otherwise need it (e.g. plain console, or a report format that doesn't render it)"
    )]
    pub capture_exchange: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReflectArgs {
    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help = "Wire protocol: grpc, grpc-web, connectrpc"
    )]
    pub protocol: String,

    #[arg(help = "Service symbol, or service/method symbol (e.g. `pkg.Service/Method`)")]
    pub symbol: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Server address (host:port); overrides $GRPCTESTIFY_ADDRESS"
    )]
    pub address: Option<String>,

    #[arg(long, default_value_t = false, help = "Plaintext connection (no TLS)")]
    pub plaintext: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip TLS certificate verification"
    )]
    pub insecure: bool,

    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text, json"
    )]
    pub format: String,

    #[arg(
        long,
        default_value_t = false,
        help = "List all methods with full signatures"
    )]
    pub list_methods: bool,

    #[arg(
        long,
        value_name = "SUBSTRING",
        help = "Only show services/methods whose name contains this substring (case-insensitive)"
    )]
    pub filter: Option<String>,

    #[arg(
        long,
        value_name = "SERVICE/METHOD",
        help = "Describe a method's request and response message fields"
    )]
    pub describe: Option<String>,

    #[arg(long, value_name = "FILE", help = "CA certificate path for TLS")]
    pub tls_ca: Option<String>,

    #[arg(long, value_name = "FILE", help = "Client certificate path for TLS")]
    pub tls_cert: Option<String>,

    #[arg(long, value_name = "FILE", help = "Client key path for TLS")]
    pub tls_key: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct FmtArgs {
    #[arg(required = true, value_name = "FILES", help = "Files to format")]
    pub files: Vec<PathBuf>,

    #[arg(
        short = 'w',
        long,
        default_value_t = false,
        help = "Write changes to file instead of stdout"
    )]
    pub write: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CallArgs {
    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help = "Wire protocol: grpc, grpc-web, connectrpc (gRPC calls only)"
    )]
    pub protocol: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Where to dial: host:port for gRPC, or an origin like https://api.example.com for HTTP; overrides the file's ADDRESS and $GRPCTESTIFY_ADDRESS"
    )]
    pub address: Option<String>,

    #[arg(help = "File to call (omit if using -e)")]
    pub file: Option<PathBuf>,

    #[arg(
        short = 'e',
        long,
        value_name = "SERVICE/METHOD",
        conflicts_with = "file",
        help = "Inline endpoint, skips file: package.Service/Method for gRPC, or `GET /path` for HTTP"
    )]
    pub endpoint: Option<String>,

    #[arg(
        short = 'd',
        long,
        value_name = "JSON",
        requires = "endpoint",
        help = "Inline request body (used with -e); JSON for gRPC, any text for HTTP"
    )]
    pub data: Option<String>,

    #[arg(
        long,
        value_name = "N",
        help = "Document index for multi-document files (1-based)"
    )]
    pub doc_index: Option<usize>,

    #[arg(
        short = 'H',
        long = "header",
        value_name = "NAME: VALUE",
        help = "Request header or gRPC metadata entry, like curl -H (repeatable)"
    )]
    pub header: Vec<String>,

    #[arg(
        short = 'i',
        long,
        default_value_t = false,
        help = "Include response headers in output, printed before body (-i)"
    )]
    pub include: bool,

    #[arg(
        short = 'v',
        long,
        default_value_t = false,
        help = "Verbose mode: show request/response metadata (-v)"
    )]
    pub verbose: bool,

    #[arg(
        long = "vv",
        default_value_t = false,
        help = "Extra verbose mode: verbose output plus timing (-vv)"
    )]
    pub very_verbose: bool,

    #[arg(
        short = 'o',
        long,
        value_name = "FILE",
        help = "Output file (stdout if omitted)"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        short = 'D',
        long,
        value_name = "FILE",
        help = "Dump response headers (and, for HTTP, the status) to a file (-D)"
    )]
    pub dump_header: Option<PathBuf>,

    #[arg(short = 's', long, default_value_t = false, help = "Silent mode (-s)")]
    pub silent: bool,

    #[arg(
        short = 'S',
        long,
        default_value_t = false,
        help = "Show errors even in silent mode (-S)"
    )]
    pub show_error: bool,

    #[arg(
        short = 'f',
        long,
        default_value_t = false,
        help = "Exit non-zero when an HTTP answer is 4xx or 5xx, like curl -f"
    )]
    pub fail: bool,

    #[arg(
        short = 'L',
        long,
        default_value_t = false,
        help = "Follow HTTP redirects, like curl -L (HTTP calls only)"
    )]
    pub location: bool,

    #[arg(
        long,
        default_value_t = 30,
        value_name = "SECS",
        help = "Connection timeout in seconds"
    )]
    pub connect_timeout: u64,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip TLS certificate verification (gRPC calls only)"
    )]
    pub insecure: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Plaintext connection (no TLS) — overrides any TLS from the file (gRPC calls only)"
    )]
    pub plaintext: bool,

    #[arg(
        long,
        value_name = "FILE",
        help = "CA certificate path for TLS (overrides the file's TLS section; gRPC calls only)"
    )]
    pub tls_ca: Option<String>,

    #[arg(
        long,
        value_name = "FILE",
        help = "Client certificate path for TLS (overrides the file's TLS section; gRPC calls only)"
    )]
    pub tls_cert: Option<String>,

    #[arg(
        long,
        value_name = "FILE",
        help = "Client key path for TLS (overrides the file's TLS section; gRPC calls only)"
    )]
    pub tls_key: Option<String>,

    #[arg(
        long,
        default_value_t = 60,
        value_name = "SECS",
        help = "Request timeout in seconds"
    )]
    pub max_time: u64,

    #[arg(
        long,
        default_value_t = false,
        help = "Run as benchmark instead of single call"
    )]
    pub bench: bool,

    #[arg(
        long,
        requires = "bench",
        value_name = "N",
        help = "Benchmark concurrency (with --bench)"
    )]
    pub concurrency: Option<u32>,

    #[arg(
        long,
        requires = "bench",
        value_name = "N",
        help = "Benchmark requests (with --bench)"
    )]
    pub requests: Option<u64>,

    #[arg(
        long,
        requires = "bench",
        value_name = "DURATION",
        help = "Benchmark duration (with --bench), e.g. \"30s\""
    )]
    pub duration: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct GenArgs {
    #[arg(
        short = 'o',
        long,
        value_name = "FILE",
        help = "Output file (stdout if omitted)"
    )]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub source: GenSource,
}

#[derive(Subcommand, Debug, Clone)]
pub enum GenSource {
    #[command(about = "Generate from grpcurl invocation")]
    Grpcurl(GenGrpcurlArgs),
}

#[derive(Args, Debug, Clone)]
#[command(trailing_var_arg = true)]
pub struct GenGrpcurlArgs {
    #[arg(
        short = 'e',
        long,
        default_value_t = false,
        help = "Execute invocation and append RESPONSE/ERROR"
    )]
    pub execute: bool,

    #[arg(
        required = true,
        allow_hyphen_values = true,
        help = "grpcurl command arguments after `gen grpcurl`"
    )]
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
            "layout" => OptimizeLevel::Layout,
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
    #[arg(
        long,
        default_value = "127.0.0.1",
        value_name = "HOST",
        help = "Host/interface to bind; loopback only by default",
        long_help = "Host/interface to bind. Defaults to loopback only; pass e.g. 0.0.0.0 to expose the playground on the network — every request then needs the token printed at startup (or GRPCTESTIFY_PLAY_TOKEN)"
    )]
    pub host: String,

    #[arg(
        long,
        default_value = "4755",
        value_name = "PORT",
        help = "Port to listen on (default: 4755)"
    )]
    pub port: u16,

    #[arg(
        long,
        default_value = ".",
        value_name = "DIR",
        help = "Directory with .gctf / .httf / .apif collections (default: current dir)"
    )]
    pub dir: std::path::PathBuf,

    #[arg(long, default_value_t = false, help = "Open browser automatically")]
    pub open: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Initialize the .grpctestify project directory and exit"
    )]
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
    fn every_subcommand_and_flag_carries_help_text() {
        use clap::CommandFactory;

        fn walk(command: &clap::Command, path: &str, missing: &mut Vec<String>) {
            for arg in command.get_arguments() {
                let id = arg.get_id().as_str();
                if id == "help" || id == "version" {
                    continue;
                }
                if arg
                    .get_help()
                    .is_none_or(|help| help.to_string().trim().is_empty())
                {
                    missing.push(format!("{path} <{id}>"));
                }
            }
            for sub in command.get_subcommands() {
                if sub.get_name() == "help" {
                    continue;
                }
                let name = format!("{path} {}", sub.get_name());
                if sub
                    .get_about()
                    .is_none_or(|about| about.to_string().trim().is_empty())
                {
                    missing.push(name.clone());
                }
                walk(sub, &name, missing);
            }
        }

        let command = Cli::command();
        let mut missing = Vec::new();
        walk(&command, "grpctestify", &mut missing);
        assert!(missing.is_empty(), "no help text for: {missing:#?}");
    }

    #[test]
    fn call_fail_flag_is_off_by_default_and_parses() {
        let cli = Cli::parse_from(["grpctestify", "call", "-e", "GET /x"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert!(!call.fail);

        let cli = Cli::parse_from(["grpctestify", "call", "-f", "-e", "GET /x"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert!(call.fail);

        let cli = Cli::parse_from(["grpctestify", "call", "--fail", "-S", "-e", "GET /x"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };
        assert!(call.fail && call.show_error);
    }

    #[test]
    fn optimize_layout_level_parses() {
        let cli = Cli::parse_from(["grpctestify", "fmt", "-O", "layout", "t.gctf"]);
        assert_eq!(
            cli.optimize_level(crate::optimizer::OptimizeLevel::Safe),
            crate::optimizer::OptimizeLevel::Layout
        );
        let cli = Cli::parse_from(["grpctestify", "fmt", "t.gctf"]);
        assert_eq!(
            cli.optimize_level(crate::optimizer::OptimizeLevel::Layout),
            crate::optimizer::OptimizeLevel::Layout
        );
    }

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
