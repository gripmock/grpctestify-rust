// Configuration file handling

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub progress: ProgressConfig,

    #[serde(default)]
    pub coverage: CoverageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Default gRPC server address
    #[serde(default = "default_address")]
    pub address: String,

    /// Number of parallel workers
    #[serde(default = "default_parallel")]
    pub parallel: String,

    /// Test timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Number of retries for failed network calls
    #[serde(default = "default_retry")]
    pub retry: u32,

    /// Initial delay between retries (seconds)
    #[serde(default = "default_retry_delay")]
    pub retry_delay: f64,

    /// Report format
    #[serde(default)]
    pub log_format: Option<String>,

    /// Output file for reports
    #[serde(default)]
    pub log_output: Option<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            parallel: default_parallel(),
            timeout: default_timeout(),
            retry: default_retry(),
            retry_delay: default_retry_delay(),
            log_format: None,
            log_output: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressConfig {
    /// Progress indicator mode
    #[serde(default = "default_progress")]
    pub mode: String,

    /// Enable colored output
    #[serde(default = "default_color")]
    pub color: bool,
}

impl Default for ProgressConfig {
    fn default() -> Self {
        Self {
            mode: default_progress(),
            color: default_color(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageConfig {
    /// Enable coverage collection
    #[serde(default)]
    pub enabled: bool,

    /// Output file for coverage report
    #[serde(default)]
    pub output: Option<String>,
}

// Default values
pub const ENV_GRPCTESTIFY_ADDRESS: &str = "GRPCTESTIFY_ADDRESS";

pub fn default_address() -> String {
    String::from("localhost:4770")
}

pub fn default_parallel() -> String {
    String::from("auto")
}

pub fn default_timeout() -> u64 {
    30
}

fn default_retry() -> u32 {
    3
}

fn default_retry_delay() -> f64 {
    1.0
}

fn default_progress() -> String {
    String::from("auto")
}

fn default_color() -> bool {
    true
}

impl Config {
    /// Load configuration from default locations
    pub fn load() -> Option<Self> {
        // Check locations in order:
        // 1. .grpctestifyrc (current directory)
        // 2. ~/.grpctestifyrc (home directory)
        // 3. .grpctestifyrc.toml (current directory)
        // 4. ~/.grpctestifyrc.toml (home directory)

        let cwd = std::env::current_dir().ok()?;
        let home = dirs::home_dir()?;

        let paths = [
            cwd.join(".grpctestifyrc"),
            home.join(".grpctestifyrc"),
            cwd.join(".grpctestifyrc.toml"),
            home.join(".grpctestifyrc.toml"),
        ];

        for path in &paths {
            if path.exists() {
                return Self::load_from_file(path);
            }
        }

        None
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::parse(&content)
    }

    /// Parse configuration from TOML string
    pub fn parse(content: &str) -> Option<Self> {
        toml::from_str(content).ok()
    }

    /// Generate default configuration as TOML
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_config() {
        let toml = r#"
[general]
address = "localhost:4770"
parallel = "4"
timeout = 30
retry = 3
retry_delay = 1.0

[progress]
mode = "bar"
color = true

[coverage]
enabled = true
output = "coverage.txt"
"#;

        let config = Config::parse(toml).expect("Failed to parse config");
        assert_eq!(config.general.address, "localhost:4770");
        assert_eq!(config.general.parallel, "4");
        assert_eq!(config.general.timeout, 30);
        assert_eq!(config.progress.mode, "bar");
        assert!(config.progress.color);
        assert!(config.coverage.enabled);
        assert_eq!(config.coverage.output, Some("coverage.txt".to_string()));
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.general.address, "localhost:4770");
        assert_eq!(config.general.parallel, "auto");
        assert_eq!(config.general.timeout, 30);
        assert_eq!(config.general.retry, 3);
        assert_eq!(config.general.retry_delay, 1.0);
        assert_eq!(config.progress.mode, "auto");
        assert!(config.progress.color);
        assert!(!config.coverage.enabled);
        assert!(config.coverage.output.is_none());
    }

    #[test]
    fn test_general_config_default() {
        let general = GeneralConfig::default();
        assert_eq!(general.address, "localhost:4770");
        assert_eq!(general.parallel, "auto");
        assert_eq!(general.timeout, 30);
        assert_eq!(general.retry, 3);
        assert_eq!(general.retry_delay, 1.0);
        assert!(general.log_format.is_none());
        assert!(general.log_output.is_none());
    }

    #[test]
    fn test_progress_config_default() {
        let progress = ProgressConfig::default();
        assert_eq!(progress.mode, "auto");
        assert!(progress.color);
    }

    #[test]
    fn test_coverage_config_default() {
        let coverage = CoverageConfig::default();
        assert!(!coverage.enabled);
        assert!(coverage.output.is_none());
    }

    #[test]
    fn test_default_values() {
        assert_eq!(default_address(), "localhost:4770");
        assert_eq!(default_parallel(), "auto");
        assert_eq!(default_timeout(), 30);
        assert_eq!(default_progress(), "auto");
        assert!(default_color());
    }

    #[test]
    fn test_parse_config_partial() {
        let toml = r#"
[general]
address = "custom:5000"
"#;

        let config = Config::parse(toml).expect("Failed to parse config");
        assert_eq!(config.general.address, "custom:5000");
        assert_eq!(config.general.parallel, "auto"); // default
    }

    #[test]
    fn test_parse_config_invalid_toml() {
        let toml = "invalid toml {{{";
        let result = Config::parse(toml);
        assert!(result.is_none());
    }

    #[test]
    fn test_config_to_toml() {
        let config = Config::default();
        let toml = config.to_toml();
        assert!(toml.contains("[general]"));
        assert!(toml.contains("[progress]"));
        assert!(toml.contains("[coverage]"));
    }

    #[test]
    fn test_config_to_toml_custom() {
        let config = Config {
            general: GeneralConfig {
                address: "test:9000".to_string(),
                parallel: "8".to_string(),
                timeout: 60,
                retry: 5,
                retry_delay: 2.0,
                log_format: Some("json".to_string()),
                log_output: Some("/tmp/report.json".to_string()),
            },
            progress: ProgressConfig {
                mode: "dots".to_string(),
                color: false,
            },
            coverage: CoverageConfig {
                enabled: true,
                output: Some("coverage.xml".to_string()),
            },
        };

        let toml = config.to_toml();
        assert!(toml.contains("test:9000"));
        assert!(toml.contains("dots"));
        assert!(toml.contains("coverage.xml"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_config_load_from_file() {
        let toml = r#"
[general]
address = "file-test:1234"
"#;

        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file
            .write_all(toml.as_bytes())
            .expect("Failed to write to temp file");

        let config = Config::load_from_file(temp_file.path()).expect("Failed to load config");
        assert_eq!(config.general.address, "file-test:1234");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_config_load_from_nonexistent_file() {
        let result = Config::load_from_file(Path::new("/nonexistent/path/config.toml"));
        assert!(result.is_none());
    }

    #[test]
    fn test_config_debug() {
        let config = Config::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("GeneralConfig"));
    }

    #[test]
    fn test_config_clone() {
        let config1 = Config::default();
        let config2 = config1.clone();
        assert_eq!(config1.general.address, config2.general.address);
    }
}
