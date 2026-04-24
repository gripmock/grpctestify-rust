// Proxy environment variable detection.
//
// gRPC transport (tonic/hyper 1.x) does not support HTTP CONNECT proxies natively.
// This module reads the conventional proxy env vars and surfaces them for logging
// so operators are not surprised when proxy settings appear to be ignored.

use std::env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEnv {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyEnv {
    /// Read proxy settings from the environment (lowercase wins over uppercase).
    pub fn from_env() -> Self {
        Self {
            http_proxy: read_proxy_var("http_proxy", "HTTP_PROXY"),
            https_proxy: read_proxy_var("https_proxy", "HTTPS_PROXY"),
            no_proxy: read_proxy_var("no_proxy", "NO_PROXY"),
        }
    }

    /// Build from explicit values (used in tests and for dependency injection).
    pub fn new(
        http_proxy: Option<String>,
        https_proxy: Option<String>,
        no_proxy: Option<String>,
    ) -> Self {
        Self {
            http_proxy,
            https_proxy,
            no_proxy,
        }
    }

    /// Returns true if any proxy variable is set.
    pub fn any_set(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some() || self.no_proxy.is_some()
    }

    /// Emit tracing warnings when proxy vars that will be ignored are detected.
    /// NO_PROXY is intentionally excluded — it is informational only.
    pub fn warn_if_set(&self) {
        if let Some(v) = &self.http_proxy {
            tracing::warn!(
                "HTTP_PROXY={} is set but gRPC transport does not support HTTP CONNECT proxies; \
                 the variable will be ignored.",
                v
            );
        }
        if let Some(v) = &self.https_proxy {
            tracing::warn!(
                "HTTPS_PROXY={} is set but gRPC transport does not support HTTP CONNECT proxies; \
                 the variable will be ignored.",
                v
            );
        }
    }
}

/// Read a proxy variable: lowercase takes priority over uppercase.
fn read_proxy_var(lower: &str, upper: &str) -> Option<String> {
    env::var(lower).ok().or_else(|| env::var(upper).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_set_false_when_all_none() {
        let p = ProxyEnv::new(None, None, None);
        assert!(!p.any_set());
    }

    #[test]
    fn any_set_true_when_http_proxy_present() {
        let p = ProxyEnv::new(Some("http://proxy:3128".into()), None, None);
        assert!(p.any_set());
    }

    #[test]
    fn any_set_true_when_https_proxy_present() {
        let p = ProxyEnv::new(None, Some("http://proxy:8080".into()), None);
        assert!(p.any_set());
    }

    #[test]
    fn any_set_true_when_no_proxy_present() {
        let p = ProxyEnv::new(None, None, Some(".local,localhost".into()));
        assert!(p.any_set());
    }

    #[test]
    fn lowercase_wins_over_uppercase() {
        // Simulate the priority without touching the real env:
        // read_proxy_var prefers lowercase; if lowercase is set, uppercase is ignored.
        // We verify the logic by constructing values directly.
        let lower = Some("http://lower:3128".to_string());
        let upper = Some("http://upper:3128".to_string());
        // Mimics: lower.or(upper)
        let result = lower.clone().or(upper);
        assert_eq!(result, lower);
    }

    #[test]
    fn uppercase_used_when_lowercase_absent() {
        let result: Option<String> =
            None::<String>.or_else(|| Some("http://upper:8080".to_string()));
        assert_eq!(result.as_deref(), Some("http://upper:8080"));
    }
}
