use clap::builder::styling::{AnsiColor, Styles};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Help styling that matches the console design language: bold-green section
/// headers, bold-cyan flags/commands, plain placeholders. anstream strips these
/// automatically when output is piped or `NO_COLOR` is set.
const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::White.on_default());

/// Progress indicator modes
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

/// Log format types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogFormat {
    Console,
    Json,
    Yaml,
    JUnit,
    Allure,
    Html,
}

/// gRPC testing utility written in Rust
#[derive(Parser, Debug)]
#[command(name = "grpctestify")]
#[command(author = "grpctestify team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(styles = HELP_STYLES)]
// The snail logo is attached at runtime in `main.rs` so it renders through the
// same colour-aware path as the welcome screen (green on TTY, plain otherwise).
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

    // Flatten RunArgs to support implicit run command at top-level.
    // This allows `grpctestify tests/` to work as expected.
    #[command(flatten)]
    pub run_args: RunArgs,

    /// Enable verbose debug output
    #[arg(short = 'v', long, global = true, default_value_t = false)]
    pub verbose: bool,

    /// Optimizer level (0=none, 1=safe, 2=advisory, 3=aggressive)
    #[arg(long = "optimize", short = 'O', value_name = "LEVEL", global = true, default_value_t = String::new())]
    pub optimize: String,

    /// Install shell completion (bash, zsh, fish, elvish, powershell)
    #[arg(long, value_name = "SHELL_TYPE", value_parser = ["bash", "zsh", "fish", "elvish", "powershell"])]
    pub completion: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub enum Commands {
    // Authoring loop — write, validate, format.
    /// Run .gctf tests (default)
    Run(Box<RunArgs>),
    /// Validate .gctf syntax & semantics
    Check(CheckArgs),
    /// Format .gctf files in place
    Fmt(FmtArgs),

    // Create tests from an existing API.
    /// Generate a .gctf from proto/descriptor/reflection
    Scaffold(ScaffoldArgs),
    /// Generate a .gctf from a captured invocation
    Gen(GenArgs),

    // Explore & understand.
    /// List services & methods via server reflection
    Reflect(ReflectArgs),
    /// Show a test's structure & metadata
    Inspect(InspectArgs),
    /// Explain a test's execution flow
    Explain(ExplainArgs),
    /// Print the equivalent grpcurl command
    Grpcurl(GrpcurlArgs),
    /// List discovered .gctf test files
    List(ListArgs),
    /// Generate Markdown API docs from .gctf test files
    Docs(DocsArgs),
    /// Visualize fixture setup/teardown topology and multi-document chains
    Graph(GraphArgs),

    // Probe a running server.
    /// Call a gRPC method without assertions
    Call(CallArgs),
    /// Check a server's gRPC health
    Health(HealthArgs),

    // Performance.
    /// Load-test endpoints
    Bench(BenchArgs),
    /// Compare two bench reports & gate regressions
    BenchCompare(BenchCompareArgs),

    // Data sources.
    /// Build & manage data-source indexes
    Index(IndexArgs),
    /// Query a data source (CSV/TSV/NDJSON)
    Query(QueryArgs),

    // Servers & tooling.
    /// Launch the web playground
    Play(PlayArgs),
    /// Run the .gctf language server (LSP)
    Lsp(LspArgs),

    /// Install/manage .rhai plugins from a git host
    Plugins(PluginsArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsArgs {
    #[command(subcommand)]
    pub action: PluginsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PluginsAction {
    /// Install a plugin from host/owner/repo[/subpath][@spec]
    Install(PluginsInstallArgs),
    /// List installed plugins
    List(PluginsListArgs),
    /// Remove an installed plugin
    Remove(PluginsRemoveArgs),
    /// Re-resolve and re-fetch installed plugins
    Update(PluginsUpdateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PluginsInstallArgs {
    /// host/owner/repo[/subpath][@spec] — spec is a tag/branch (exact) or
    /// a semver range like ^1.2.0 (omit for the highest semver tag, or HEAD
    /// if the repo has none)
    pub source: String,

    /// Install to $HOME/.grpctestify instead of the project-local
    /// ./.grpctestify
    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsListArgs {
    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,

    /// List both the project-local and user-global tiers
    #[arg(long, default_value_t = false)]
    pub all: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsRemoveArgs {
    /// host/owner/repo, as shown by `plugins list`
    pub name: String,

    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PluginsUpdateArgs {
    /// host/owner/repo to update (omit to update every installed plugin in the tier)
    pub name: Option<String>,

    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ScaffoldArgs {
    /// Fully-qualified method to scaffold (package.Service/Method)
    #[arg(long, value_name = "SERVICE/METHOD", required = true)]
    pub endpoint: String,

    /// Proto file or directory to compile (pure-Rust protox, no protoc)
    #[arg(long, value_name = "FILE_OR_DIR")]
    pub proto: Option<PathBuf>,

    /// Pre-compiled FileDescriptorSet (.protoset/.pb)
    #[arg(long, value_name = "FILE")]
    pub descriptor: Option<PathBuf>,

    /// Load descriptors from server reflection at --address
    #[arg(long, default_value_t = false)]
    pub reflect: bool,

    /// Server address (host:port)
    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    /// Output file (stdout if omitted)
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Overwrite the output file if it already exists
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Use TLS with certificate verification
    #[arg(long, default_value_t = false)]
    pub tls: bool,

    /// Skip TLS certificate verification
    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    /// Plaintext connection (no TLS)
    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,
}

#[derive(Args, Debug, Clone)]
pub struct HealthArgs {
    /// Server address (host:port)
    #[arg(required = true, value_name = "ADDRESS")]
    pub address: String,

    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    /// Service name to check (repeatable; default: none — checks overall server health)
    #[arg(long, value_name = "NAME")]
    pub service: Vec<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    /// Use TLS with certificate verification
    #[arg(long, default_value_t = false)]
    pub tls: bool,

    /// Skip TLS certificate verification
    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    /// Timeout in seconds (per health-check RPC)
    #[arg(long, default_value_t = 10, value_name = "SECS")]
    pub timeout: u64,

    /// Poll until healthy instead of checking once (for CI readiness gating)
    #[arg(long, default_value_t = false)]
    pub watch: bool,

    /// Poll interval in seconds for --watch
    #[arg(long, default_value_t = 1.0, value_name = "SECS")]
    pub interval: f64,

    /// Give up waiting after this many seconds in --watch mode
    #[arg(long, default_value_t = 60.0, value_name = "SECS")]
    pub watch_timeout: f64,
}

#[derive(Args, Debug, Clone)]
pub struct GrpcurlArgs {
    /// File to convert
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    /// Document index for multi-document .gctf files (1-based)
    #[arg(long)]
    pub doc_index: Option<usize>,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    /// Use stdio for communication (default)
    #[arg(long, default_value_t = true)]
    pub stdio: bool,
}

#[derive(Args, Debug, Clone)]
pub struct BenchArgs {
    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    /// Test files or directories to benchmark
    #[arg(required = true, value_name = "PATH")]
    pub test_paths: Vec<PathBuf>,

    /// Benchmark profile (functional, load, stress, spike, soak)
    #[arg(long, value_name = "PROFILE")]
    pub profile: Option<String>,

    /// Benchmark mode (fixed, stepping, adaptive)
    #[arg(long, value_name = "MODE")]
    pub mode: Option<String>,

    /// Number of concurrent workers
    #[arg(short = 'c', long, value_name = "N")]
    pub concurrency: Option<u32>,

    /// Total number of requests to send
    #[arg(short = 'n', long, value_name = "N")]
    pub requests: Option<u64>,

    /// Duration of benchmark (e.g., 30s, 5m)
    #[arg(short = 'd', long, value_name = "DURATION")]
    pub duration: Option<String>,

    /// Ramp-up duration before steady-state load (e.g., 10s)
    #[arg(long = "ramp-up", alias = "ramp_up", value_name = "DURATION")]
    pub ramp_up: Option<String>,

    /// Warmup period excluded from final metrics (e.g., 5s)
    #[arg(long, value_name = "DURATION")]
    pub warmup: Option<String>,

    /// Maximum runtime with request-count mode (e.g., 30s, 5m)
    #[arg(long, value_name = "DURATION")]
    pub max_duration: Option<String>,

    /// Maximum requests per second (rate limit)
    #[arg(long, value_name = "RPS")]
    pub max_rps: Option<f64>,

    /// Load schedule strategy (const, step, line)
    #[arg(long = "load-schedule", value_name = "SCHEDULE")]
    pub load_schedule: Option<String>,

    /// Starting RPS for step/line load schedules
    #[arg(long = "load-start", value_name = "RPS")]
    pub load_start: Option<f64>,

    /// Step/slope RPS delta for step/line schedules
    #[arg(long = "load-step", value_name = "RPS_DELTA")]
    pub load_step: Option<f64>,

    /// Optional ending RPS for step/line schedules
    #[arg(long = "load-end", value_name = "RPS")]
    pub load_end: Option<f64>,

    /// Duration of each step for step schedule
    #[arg(long = "load-step-duration", value_name = "DURATION")]
    pub load_step_duration: Option<String>,

    /// Maximum duration of load adjustments
    #[arg(long = "load-max-duration", value_name = "DURATION")]
    pub load_max_duration: Option<String>,

    /// Number of gRPC connections to use (<= concurrency)
    #[arg(long, value_name = "N")]
    pub connections: Option<u32>,

    /// Connection timeout (e.g., 10s)
    #[arg(long, value_name = "DURATION")]
    pub connect_timeout: Option<String>,

    /// Per-request timeout (e.g., 120s); defaults to the benchmark duration
    #[arg(long, value_name = "DURATION")]
    pub request_timeout: Option<String>,

    /// Keepalive interval (e.g., 30s)
    #[arg(long, value_name = "DURATION")]
    pub keepalive: Option<String>,

    /// Number of CPU cores to use
    #[arg(long, value_name = "N")]
    pub cpus: Option<usize>,

    /// User-defined benchmark run name
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Assertion mode (fail_fast, collect_all, skip)
    #[arg(long, visible_alias = "bench-assert-mode", value_name = "MODE")]
    pub assert_mode: Option<String>,

    /// Disable ASSERTS evaluation to measure transport baseline
    #[arg(long, visible_alias = "bench-no-assert", default_value_t = false)]
    pub no_assert: bool,

    /// Sample rate for detailed logging (0.0-1.0)
    #[arg(long, value_name = "RATE")]
    pub sample_rate: Option<f64>,

    /// Enable reflection/proto caching
    #[arg(long)]
    pub cache: Option<bool>,

    /// Skip first N requests in latency metrics
    #[arg(long, value_name = "N")]
    pub skip_first: Option<u32>,

    /// Include errors in latency calculation
    #[arg(long)]
    pub count_errors_in_latency: Option<bool>,

    /// In-flight handling when duration limit is reached (close, wait, ignore)
    #[arg(long, value_name = "MODE")]
    pub duration_stop: Option<String>,

    /// Latency percentiles to report (comma-separated, e.g. p50,p90,p95,p99)
    #[arg(long, value_name = "LIST")]
    pub latency_percentiles: Option<String>,

    /// Progress heartbeat interval (e.g. 5s)
    #[arg(long = "progress-interval", value_name = "DURATION")]
    pub progress_interval: Option<String>,

    /// Report format: console, json, csv, ndjson, prometheus
    #[arg(
        long = "log-format",
        visible_alias = "bench-format",
        default_value = "console",
        value_name = "FORMAT"
    )]
    pub format: String,

    /// Output file for the report
    #[arg(
        short = 'o',
        long = "log-output",
        visible_alias = "bench-output",
        value_name = "FILE"
    )]
    pub output: Option<PathBuf>,

    /// Custom MiniJinja template file for benchmark report
    #[arg(long, value_name = "TEMPLATE_FILE")]
    pub report_template: Option<PathBuf>,

    /// Allure output directory for benchmark attachments
    #[arg(long, value_name = "DIR")]
    pub allure_output_dir: Option<PathBuf>,

    /// Compact console output (omit histogram)
    #[arg(long, default_value_t = false)]
    pub compact: bool,

    /// Only run tests carrying ALL of these tags (repeatable)
    #[arg(long = "tags", value_name = "TAG")]
    pub tags: Vec<String>,

    /// Skip tests carrying ANY of these tags (repeatable)
    #[arg(long = "skip-tags", value_name = "TAG")]
    pub skip_tags: Vec<String>,

    /// Exclude paths matching this glob (repeatable)
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// List available benchmark profiles and exit
    #[arg(long, default_value_t = false)]
    pub list_profiles: bool,

    /// Path to custom profile YAML file
    #[arg(long, value_name = "FILE")]
    pub profile_file: Option<PathBuf>,

    /// Direct gRPC method call (service/method) — no .gctf file needed
    #[arg(long, value_name = "SERVICE/METHOD")]
    pub call: Option<String>,

    /// Inline JSON request body (used with --call)
    #[arg(long, value_name = "JSON")]
    pub data: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct BenchCompareArgs {
    /// Baseline bench report (JSON produced by `bench --log-format json`)
    #[arg(required = true, value_name = "FILE")]
    pub baseline: PathBuf,

    /// Current bench report to compare against the baseline
    #[arg(required = true, value_name = "FILE")]
    pub current: PathBuf,

    /// Max tolerated latency rise (percent) for mean and each percentile
    #[arg(long, value_name = "PCT", default_value_t = 10.0)]
    pub max_latency_regression: f64,

    /// Max tolerated error-rate rise, in percentage points
    #[arg(long, value_name = "POINTS", default_value_t = 1.0)]
    pub max_error_rate_regression: f64,

    /// Max tolerated throughput (rps) drop (percent) before failing
    #[arg(long, value_name = "PCT", default_value_t = 5.0)]
    pub min_throughput: f64,

    /// Report format: console, json
    #[arg(long, default_value = "console", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct IndexArgs {
    /// .gctf file(s) or directory with BENCH.sources definitions
    #[arg(required = true, value_name = "PATH")]
    pub sources: Vec<PathBuf>,

    /// Force rebuild of all required indexes
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Show index file statistics
    #[arg(long, default_value_t = false)]
    pub stats: bool,
}

#[derive(Args, Debug, Clone)]
pub struct QueryArgs {
    /// Files or directories to query (default: interactive shell)
    #[arg(required = false, value_name = "PATH")]
    pub files: Vec<PathBuf>,

    /// Query expression to execute
    #[arg(short = 'q', long, value_name = "EXPR")]
    pub query: Option<String>,

    /// Run in interactive shell mode
    #[arg(short = 's', long, default_value_t = false)]
    pub shell: bool,

    /// Index column for direct file mode
    #[arg(short = 'i', long, value_name = "COLUMN")]
    pub indexed_by: Option<String>,

    /// Output format: json, csv, table, line, tsv
    #[arg(short = 'f', long, default_value = "table", value_name = "FORMAT")]
    pub format: String,

    /// Maximum number of rows to return
    #[arg(short = 'n', long, value_name = "N")]
    pub limit: Option<usize>,

    /// Skip N rows
    #[arg(short = 'o', long, value_name = "N")]
    pub offset: Option<usize>,

    /// Output columns (comma-separated)
    #[arg(short = 'c', long, value_name = "COLS")]
    pub columns: Option<String>,

    /// Sort by column (prefix with - for DESC)
    #[arg(long, value_name = "COLUMN")]
    pub order_by: Option<String>,

    /// Output file (stdout if omitted)
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Skip header row in output
    #[arg(long, default_value_t = false)]
    pub no_header: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    /// Test file or directory to list
    #[arg(required = false, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Output format: text, json
    #[arg(long, default_value = "json", value_name = "FORMAT")]
    pub format: String,

    /// Include test range information
    #[arg(long, default_value_t = false)]
    pub with_range: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DocsArgs {
    /// Test files or directories to document (defaults to the current directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Directory to write the generated Markdown into
    #[arg(long, short = 'o', default_value = "docs/api", value_name = "DIR")]
    pub output: PathBuf,

    /// Embed method/field coverage from a prior `run --coverage --coverage-format json` report
    #[arg(long, value_name = "PATH")]
    pub coverage: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct GraphArgs {
    /// Test files or directories to visualize (defaults to the current directory)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Output format: text, mermaid
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    /// File to inspect
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct ExplainArgs {
    /// File to explain
    #[arg(required = true, value_name = "FILE")]
    pub file: PathBuf,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    /// Post-hoc mode: JSON report from a prior `run --log-format json` to
    /// correlate against this file's plan (actual per-assertion pass/fail +
    /// timing, instead of just the static/optimized plan).
    #[arg(long, value_name = "REPORT_JSON")]
    pub against: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Files to validate
    #[arg(required = true, value_name = "FILES")]
    pub files: Vec<PathBuf>,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    /// Validate BENCH section configuration
    #[arg(long, default_value_t = false)]
    pub bench: bool,
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// Test files or directories to run (defaults to the current directory)
    // Optional so it doesn't clash with subcommand names when parsed at the top
    // level; emptiness is enforced manually where a path is required.
    #[arg(required = false, value_name = "PATH")]
    pub test_paths: Vec<PathBuf>,

    /// Exclude paths matching this glob (repeatable)
    #[arg(long = "exclude", value_name = "GLOB", help_heading = "Test Selection")]
    pub exclude: Vec<String>,

    /// Only run tests carrying ALL of these tags (repeatable)
    #[arg(long = "tags", value_name = "TAG", help_heading = "Test Selection")]
    pub tags: Vec<String>,

    /// Skip tests carrying ANY of these tags (repeatable)
    #[arg(
        long = "skip-tags",
        value_name = "TAG",
        help_heading = "Test Selection"
    )]
    pub skip_tags: Vec<String>,

    /// Order tests before running: path, size, name
    #[arg(
        short = 's',
        long,
        default_value = "path",
        value_name = "MODE",
        help_heading = "Test Selection"
    )]
    pub sort: String,

    /// Data source (CSV/TSV/NDJSON) driving each .gctf as a template — one case
    /// per row (`{{source.column}}` substitution)
    #[arg(long, value_name = "PATH", help_heading = "Test Selection")]
    pub data: Option<PathBuf>,

    /// Override the --data format (csv, tsv, ndjson); inferred from the extension otherwise
    #[arg(
        long,
        value_name = "FORMAT",
        requires = "data",
        help_heading = "Test Selection"
    )]
    pub data_format: Option<String>,

    /// Only run tests whose file content differs from --since (git, no shell-out)
    #[arg(long, default_value_t = false, help_heading = "Test Selection")]
    pub only_changed: bool,

    /// Git ref --only-changed compares against (default: HEAD, i.e. uncommitted changes)
    #[arg(
        long,
        default_value = "HEAD",
        value_name = "REF",
        requires = "only_changed",
        help_heading = "Test Selection"
    )]
    pub since: String,

    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(
        long,
        default_value = "grpc",
        value_name = "PROTOCOL",
        help_heading = "Execution"
    )]
    pub protocol: String,

    /// Worker count for parallel runs, or `auto`
    #[arg(
        short = 'p',
        long,
        default_value = "auto",
        value_name = "N",
        help_heading = "Execution"
    )]
    pub parallel: String,

    /// Per-test timeout in seconds
    #[arg(
        short = 't',
        long,
        default_value_t = 30,
        value_name = "SECS",
        help_heading = "Execution"
    )]
    pub timeout: u64,

    /// Retry failed network calls up to N times
    #[arg(
        short = 'r',
        long,
        default_value_t = 0,
        value_name = "N",
        help_heading = "Execution"
    )]
    pub retry: u32,

    /// Initial delay between retries in seconds
    #[arg(
        long,
        default_value_t = 1.0,
        value_name = "SECS",
        help_heading = "Execution"
    )]
    pub retry_delay: f64,

    /// Disable retries entirely
    #[arg(long, default_value_t = false, help_heading = "Execution")]
    pub no_retry: bool,

    /// Skip assertions and print raw server responses
    #[arg(long, default_value_t = false, help_heading = "Execution")]
    pub no_assert: bool,

    /// Snapshot mode: write actual server responses back into the test files
    #[arg(short = 'w', long, default_value_t = false, help_heading = "Execution")]
    pub write: bool,

    /// Show what would run without executing anything
    #[arg(short = 'd', long, default_value_t = false, help_heading = "Execution")]
    pub dry_run: bool,

    /// Report format: junit, json, yaml, allure, html (comma-separated for several, e.g. junit,html)
    #[arg(long, value_name = "FORMAT", help_heading = "Output & Reports")]
    pub log_format: Option<String>,

    /// Destination for --log-format: exact file (or directory, for allure) for
    /// one format; a directory holding one file per format for several
    #[arg(long, value_name = "PATH", help_heading = "Output & Reports")]
    pub log_output: Option<PathBuf>,

    /// Emit streaming NDJSON events (for IDE/CI integration)
    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub stream: bool,

    /// Progress style: auto, dots, verbose, none
    #[arg(
        long,
        default_value = "auto",
        value_name = "STYLE",
        help_heading = "Output & Reports"
    )]
    pub progress: String,

    /// Report proto API coverage after the run
    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub coverage: bool,

    /// Coverage format: text, json, html
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help_heading = "Output & Reports"
    )]
    pub coverage_format: String,

    /// Force-capture the request/response exchange even when the active
    /// reporter wouldn't otherwise need it (e.g. plain console, or a report
    /// format that doesn't render it)
    #[arg(long, default_value_t = false, help_heading = "Output & Reports")]
    pub capture_exchange: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReflectArgs {
    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    /// Service symbol, or service/method symbol (e.g. `pkg.Service/Method`)
    pub symbol: Option<String>,

    /// Server address (host:port); overrides $GRPCTESTIFY_ADDRESS
    #[arg(long, value_name = "ADDRESS")]
    pub address: Option<String>,

    /// Plaintext connection (no TLS)
    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    /// Skip TLS certificate verification
    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub format: String,

    /// List all methods with full signatures
    #[arg(long, default_value_t = false)]
    pub list_methods: bool,

    /// Only show services/methods whose name contains this substring
    /// (case-insensitive)
    #[arg(long, value_name = "SUBSTRING")]
    pub filter: Option<String>,

    /// Describe a method's request and response message fields
    #[arg(long, value_name = "SERVICE/METHOD")]
    pub describe: Option<String>,

    /// CA certificate path for TLS
    #[arg(long)]
    pub tls_ca: Option<String>,

    /// Client certificate path for TLS
    #[arg(long)]
    pub tls_cert: Option<String>,

    /// Client key path for TLS
    #[arg(long)]
    pub tls_key: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct FmtArgs {
    /// Files to format
    #[arg(required = true, value_name = "FILES")]
    pub files: Vec<PathBuf>,

    /// Write changes to file instead of stdout
    #[arg(short = 'w', long, default_value_t = false)]
    pub write: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CallArgs {
    /// Wire protocol: grpc, grpc-web, connectrpc
    #[arg(long, default_value = "grpc", value_name = "PROTOCOL")]
    pub protocol: String,

    /// File to call (omit if using -e)
    pub file: Option<PathBuf>,

    /// Inline endpoint (package.Service/Method), skips file
    #[arg(
        short = 'e',
        long,
        value_name = "SERVICE/METHOD",
        conflicts_with = "file"
    )]
    pub endpoint: Option<String>,

    /// Inline JSON request body (used with -e)
    #[arg(short = 'd', long, value_name = "JSON", requires = "endpoint")]
    pub data: Option<String>,

    /// Document index for multi-document .gctf files (1-based)
    #[arg(long)]
    pub doc_index: Option<usize>,

    /// Include response headers in output, printed before body (-i)
    #[arg(short = 'i', long, default_value_t = false)]
    pub include: bool,

    /// Verbose mode: show request/response metadata (-v)
    #[arg(short = 'v', long, default_value_t = false)]
    pub verbose: bool,

    /// Extra verbose mode: verbose output plus timing (-vv)
    #[arg(long = "vv", default_value_t = false)]
    pub very_verbose: bool,

    /// Output file (stdout if omitted)
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Dump response headers to file (-D)
    #[arg(short = 'D', long)]
    pub dump_header: Option<PathBuf>,

    /// Silent mode (-s)
    #[arg(short = 's', long, default_value_t = false)]
    pub silent: bool,

    /// Show errors (-S)
    #[arg(short = 'S', long, default_value_t = false)]
    pub show_error: bool,

    /// Connection timeout in seconds
    #[arg(long, default_value_t = 30, value_name = "SECS")]
    pub connect_timeout: u64,

    /// Skip TLS certificate verification
    #[arg(long, default_value_t = false)]
    pub insecure: bool,

    /// Plaintext connection (no TLS) — overrides any TLS from the file
    #[arg(long, default_value_t = false)]
    pub plaintext: bool,

    /// CA certificate path for TLS (overrides the file's TLS section)
    #[arg(long)]
    pub tls_ca: Option<String>,

    /// Client certificate path for TLS (overrides the file's TLS section)
    #[arg(long)]
    pub tls_cert: Option<String>,

    /// Client key path for TLS (overrides the file's TLS section)
    #[arg(long)]
    pub tls_key: Option<String>,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 60, value_name = "SECS")]
    pub max_time: u64,

    /// Run as benchmark instead of single call
    #[arg(long, default_value_t = false)]
    pub bench: bool,

    /// Benchmark concurrency (with --bench)
    #[arg(long, requires = "bench")]
    pub concurrency: Option<u32>,

    /// Benchmark requests (with --bench)
    #[arg(long, requires = "bench")]
    pub requests: Option<u64>,

    /// Benchmark duration (with --bench), e.g. "30s"
    #[arg(long, requires = "bench")]
    pub duration: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct GenArgs {
    /// Output file (stdout if omitted)
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub source: GenSource,
}

#[derive(Subcommand, Debug, Clone)]
pub enum GenSource {
    /// Generate from grpcurl invocation
    Grpcurl(GenGrpcurlArgs),
}

#[derive(Args, Debug, Clone)]
#[command(trailing_var_arg = true)]
pub struct GenGrpcurlArgs {
    /// Execute invocation and append RESPONSE/ERROR
    #[arg(short = 'e', long, default_value_t = false)]
    pub execute: bool,

    /// grpcurl command arguments after `gen grpcurl`
    #[arg(required = true, allow_hyphen_values = true)]
    pub grpcurl_args: Vec<String>,
}

impl Cli {
    /// Get parallel job count (auto-detect if set to "auto")
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
            // Clamp to at least 1: buffer_unordered(0) never polls (deadlock).
            parallel.parse().unwrap_or(1).max(1)
        }
    }

    /// Get progress mode
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

    /// Get log format (first one, when several were requested)
    pub fn log_format_mode(&self) -> Option<LogFormat> {
        self.log_format_modes().into_iter().next()
    }

    /// Get every requested log format, in order, de-duplicated. `--log-format
    /// junit,html` produces both file reports from a single run.
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

    /// Get optimizer level from CLI flag or default for command
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

    /// Helper to get effective RunArgs
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

/// Trait for CLI argument types that have a `--format` option.
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
    /// Host/interface to bind. Defaults to loopback only; pass e.g. 0.0.0.0
    /// to expose the playground on the network (no auth — trusted networks only)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on (default: 4755)
    #[arg(long, default_value = "4755")]
    pub port: u16,

    /// Directory with .gctf collections (default: current dir)
    #[arg(long, default_value = ".")]
    pub dir: std::path::PathBuf,

    /// Open browser automatically
    #[arg(long, default_value_t = false)]
    pub open: bool,

    /// Initialize .grpctestify project directory
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
