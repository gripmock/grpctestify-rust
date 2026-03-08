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

    /// Disable colored output
    #[arg(short = 'c', long, global = true, default_value_t = false)]
    pub no_color: bool,

    /// Show current configuration and exit
    #[arg(long, default_value_t = false)]
    pub config: bool,

    /// Create default configuration file
    #[arg(long, value_name = "CONFIG_FILE")]
    pub init_config: Option<PathBuf>,

    /// Install shell completion (bash, zsh, fish, elvish, powershell)
    #[arg(long, value_name = "SHELL_TYPE", value_parser = ["bash", "zsh", "fish", "elvish", "powershell"])]
    pub completion: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run tests (default)
    Run(RunArgs),

    /// Explain test execution workflow
    Explain(ExplainArgs),

    /// Inspect .gctf file structure (AST and Workflow)
    Inspect(InspectArgs),

    /// List test files (for IDE test discovery)
    List(ListArgs),

    /// Reflect on server metadata (uses gRPC Server Reflection Protocol)
    Reflect(ReflectArgs),

    /// Format .gctf files
    Fmt(FmtArgs),

    /// Check .gctf file syntax and structure
    Check(CheckArgs),

    /// Start LSP server for IDE integration
    Lsp(LspArgs),
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

impl ListArgs {
    pub fn is_json(&self) -> bool {
        is_json_format(&self.format)
    }
}

impl InspectArgs {
    pub fn is_json(&self) -> bool {
        is_json_format(&self.format)
    }
}

impl ExplainArgs {
    pub fn is_json(&self) -> bool {
        is_json_format(&self.format)
    }
}

impl CheckArgs {
    pub fn is_json(&self) -> bool {
        is_json_format(&self.format)
    }
}

impl RunArgs {
    pub fn is_json_coverage(&self) -> bool {
        is_json_format(&self.coverage_format)
    }
}
