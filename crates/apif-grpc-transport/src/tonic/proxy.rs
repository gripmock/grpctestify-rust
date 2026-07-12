use std::env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEnv {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl ProxyEnv {
    pub fn from_env() -> Self {
        Self {
            http_proxy: read_proxy_var("http_proxy", "HTTP_PROXY"),
            https_proxy: read_proxy_var("https_proxy", "HTTPS_PROXY"),
            no_proxy: read_proxy_var("no_proxy", "NO_PROXY"),
        }
    }
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
    pub fn any_set(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some() || self.no_proxy.is_some()
    }
    pub fn warn_if_set(&self) {
        if let Some(v) = &self.http_proxy {
            tracing::warn!(
                "HTTP_PROXY={} is set but gRPC transport does not support HTTP CONNECT proxies; ignored.",
                v
            );
        }
        if let Some(v) = &self.https_proxy {
            tracing::warn!(
                "HTTPS_PROXY={} is set but gRPC transport does not support HTTP CONNECT proxies; ignored.",
                v
            );
        }
    }
}

fn read_proxy_var(lower: &str, upper: &str) -> Option<String> {
    env::var(lower).ok().or_else(|| env::var(upper).ok())
}
