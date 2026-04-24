// CLI argument definitions using Clap

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Progress indicator modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Dots,
    Bar,
    None,
    Verbose,
}

impl std::str::FromStr for ProgressMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dots" => Ok(Self::Dots),
            "bar" => Ok(Self::Bar),
            "none" => Ok(Self::None),
            _ => Ok(Self::Dots),
        }
    }
}

/// Log format types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Console,
    Json,
    JUnit,
    Allure,
}

/// gRPC testing utility written in Rust
#[derive(Parser, Debug)]
#[command(name = "grpctestify")]
#[command(author = "grpctestify team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Test gRPC services with simple .gctf files", long_about = None)]
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

    /// Install shell completion (bash, zsh, fish, elvish, powershell)
    #[arg(long, value_name = "SHELL_TYPE", value_parser = ["bash", "zsh", "fish", "elvish", "powershell"])]
    pub completion: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run tests (default)
    Run(Box<RunArgs>),
    /// Call gRPC endpoint without assertions
    Call(CallArgs),
    /// Generate .gctf file from external invocations
    Gen(GenArgs),
    /// Reflect gRPC service and list methods
    Reflect(ReflectArgs),
    /// Format files
    Fmt(FmtArgs),
    /// Validate files
    Check(CheckArgs),
    /// Show test information
    Inspect(InspectArgs),
    /// Explain test execution flow
    Explain(ExplainArgs),
    /// Generate grpcurl invocation from a .gctf file
    Grpcurl(GrpcurlArgs),
    /// List discovered .gctf test files
    List(ListArgs),
    /// LSP server
    Lsp(LspArgs),
}

#[derive(Args, Debug, Clone)]
pub struct GrpcurlArgs {
    /// File to convert into grpcurl command
    #[arg(required = true)]
    pub file: PathBuf,

    /// Document index for multi-document .gctf files (1-based)
    #[arg(long)]
    pub doc_index: Option<usize>,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    /// Use stdio for communication (default)
    #[arg(long, default_value_t = true)]
    pub stdio: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    /// Path to test file or directory to list
    #[arg(required = false)]
    pub path: Option<PathBuf>,

    /// Output format (text, json)
    #[arg(long, default_value = "json")]
    pub format: String,

    /// Include test range information
    #[arg(long, default_value_t = false)]
    pub with_range: bool,
}

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    /// File to inspect
    #[arg(required = true)]
    pub file: PathBuf,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct ExplainArgs {
    /// File to explain
    #[arg(required = true)]
    pub file: PathBuf,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Files to validate
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// Path to test file or directory to execute
    // We make this optional so it doesn't conflict with subcommands when parsed at top level,
    // but we'll enforce it manually if no subcommand is present.
    // However, if we use `flatten` at top level, and `subcommand` is optional,
    // Clap might be ambiguous if `test_paths` matches a subcommand name.
    // But since `test_paths` are files/dirs, usually they won't clash with "run", "reflect", etc.
    // We remove `required` constraint here and handle validation manually.
    #[arg(required = false)]
    pub test_paths: Vec<PathBuf>,

    /// Exclude files/directories matching the given glob pattern (can be used multiple times)
    #[arg(long = "exclude", value_name = "PATTERN")]
    pub exclude: Vec<String>,

    /// Filter by tags (AND - file must have ALL tags)
    #[arg(long = "tags", value_name = "TAGS")]
    pub tags: Vec<String>,

    /// Skip files with these tags (NOT OR - exclude if ANY matches)
    #[arg(long = "skip-tags", value_name = "TAGS")]
    pub skip_tags: Vec<String>,

    /// Run tests in parallel with N workers
    #[arg(short = 'p', long, default_value = "auto")]
    pub parallel: String,

    /// Show commands that would be executed without running them
    #[arg(short = 'd', long, default_value_t = false)]
    pub dry_run: bool,

    /// Sort test files by type
    #[arg(short = 's', long, default_value = "path")]
    pub sort: String,

    /// Generate test reports in specified format
    #[arg(long, value_name = "FORMAT")]
    pub log_format: Option<String>,

    /// Output file for test reports (use with --log-format)
    #[arg(long, value_name = "OUTPUT_FILE")]
    pub log_output: Option<PathBuf>,

    /// Output streaming JSON events (for IDE integration)
    #[arg(long, default_value_t = false)]
    pub stream: bool,

    /// Set timeout for individual tests (seconds)
    #[arg(short = 't', long, default_value_t = 30)]
    pub timeout: u64,

    /// Number of retries for failed network calls
    #[arg(short = 'r', long, default_value_t = 0)]
    pub retry: u32,

    /// Initial delay between retries (seconds)
    #[arg(long, default_value_t = 1.0)]
    pub retry_delay: f64,

    /// Disable retry mechanisms completely
    #[arg(long, default_value_t = false)]
    pub no_retry: bool,

    /// Progress indicator style
    #[arg(long, default_value = "auto")]
    pub progress: String,

    /// Skip assertion evaluation and print raw server responses
    #[arg(long, default_value_t = false)]
    pub no_assert: bool,

    /// Generate Proto API coverage report
    #[arg(long, default_value_t = false)]
    pub coverage: bool,

    /// Coverage output format (text, json)
    #[arg(long, default_value = "text")]
    pub coverage_format: String,

    /// Write/Overwrite test files with actual server responses (Snapshot Mode)
    #[arg(short = 'w', long, default_value_t = false)]
    pub write: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ReflectArgs {
    /// Service/method symbol OR .gctf file path
    pub symbol: Option<String>,

    /// Server address (overrides environment variable)
    #[arg(long)]
    pub address: Option<String>,

    /// Plaintext connection (no TLS). If omitted, localhost/http addresses default to plaintext.
    #[arg(long, default_value_t = false)]
    pub plaintext: bool,
}

#[derive(Args, Debug, Clone)]
pub struct FmtArgs {
    /// Files to format
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Write changes to file instead of stdout
    #[arg(short = 'w', long, default_value_t = false)]
    pub write: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CallArgs {
    /// File to call
    #[arg(required = true)]
    pub file: PathBuf,

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

    /// Output to file instead of stdout (-o)
    #[arg(short = 'o', long)]
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
    #[arg(long, default_value_t = 30)]
    pub connect_timeout: u64,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 60)]
    pub max_time: u64,
}

#[derive(Args, Debug, Clone)]
pub struct GenArgs {
    /// Output file for generated gctf (stdout if omitted)
    #[arg(short = 'o', long)]
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
            // Auto-detect CPU count
            std::thread::available_parallelism()
                .ok()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            parallel.parse().unwrap_or(1)
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
            "bar" => ProgressMode::Bar,
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

    /// Get log format
    pub fn log_format_mode(&self) -> Option<LogFormat> {
        let log_format = match &self.command {
            Some(Commands::Run(args)) => &args.log_format,
            _ => &self.run_args.log_format,
        };

        log_format.as_ref().map(|fmt| match fmt.as_str() {
            "junit" => LogFormat::JUnit,
            "json" => LogFormat::Json,
            "allure" => LogFormat::Allure,
            _ => LogFormat::Console,
        })
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

impl RunArgs {
    pub fn is_json_coverage(&self) -> bool {
        is_json_format(&self.coverage_format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_call_defaults() {
        let cli = Cli::parse_from(["grpctestify", "call", "test.gctf"]);
        let Some(Commands::Call(call)) = cli.command else {
            panic!("expected call command");
        };

        assert_eq!(call.file, PathBuf::from("test.gctf"));
        assert_eq!(call.doc_index, None);
        assert!(!call.include);
        assert!(!call.verbose);
        assert!(!call.very_verbose);
        assert!(!call.silent);
        assert!(!call.show_error);
        assert_eq!(call.connect_timeout, 30);
        assert_eq!(call.max_time, 60);
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
}
