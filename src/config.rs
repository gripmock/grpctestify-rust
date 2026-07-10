// Configuration file handling

use serde::{Deserialize, Serialize};

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

// Environment variable names
pub const ENV_GRPCTESTIFY_ADDRESS: &str = "GRPCTESTIFY_ADDRESS";
pub const ENV_GRPCTESTIFY_COMPRESSION: &str = "GRPCTESTIFY_COMPRESSION";

pub const ENV_GRPCTESTIFY_TLS_CERT_FILE: &str = "GRPCTESTIFY_TLS_CERT_FILE";
pub const ENV_GRPCTESTIFY_TLS_KEY_FILE: &str = "GRPCTESTIFY_TLS_KEY_FILE";
pub const ENV_GRPCTESTIFY_TLS_CA_FILE: &str = "GRPCTESTIFY_TLS_CA_FILE";
pub const ENV_GRPCTESTIFY_TLS_SERVER_NAME: &str = "GRPCTESTIFY_TLS_SERVER_NAME";

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

#[cfg(test)]
mod tests {
    use super::*;

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
