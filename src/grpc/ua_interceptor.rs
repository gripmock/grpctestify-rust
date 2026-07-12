use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::HeaderValue;
use tonic::body::Body;
use tower::Service;

const TONIC_UA_PREFIX: &str = " tonic/";

/// Tower middleware that strips the " tonic/{version}" suffix from User-Agent
/// that tonic always appends. Wraps any tonic-compatible service.
#[derive(Clone)]
pub struct StripTonicUA<S> {
    inner: S,
}

impl<S> StripTonicUA<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, ResBody> Service<http::Request<Body>> for StripTonicUA<S>
where
    S: Service<http::Request<Body>, Response = http::Response<ResBody>>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Future + Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: http::Request<Body>) -> Self::Future {
        // tonic always appends " tonic/{version}" to any User-Agent set via
        // gRPC metadata. Strip only that trailing suffix so callers get full
        // control over the exact UA value. If the UA doesn't end with the
        // tonic suffix (e.g. no metadata UA was set and tonic used its own
        // default), leave it untouched.
        if let Some(ua) = req.headers_mut().get(http::header::USER_AGENT)
            && let Ok(s) = ua.to_str()
            && let Some(pos) = s.rfind(TONIC_UA_PREFIX)
            && pos > 0
        {
            // Only strip if the suffix looks like " tonic/<version>"
            // (alphanumeric, dots, slashes after the prefix).
            let suffix = &s[pos..];
            let rest = suffix.strip_prefix(TONIC_UA_PREFIX).unwrap_or(suffix);
            if !rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '/')
                && let Ok(val) = HeaderValue::from_str(&s[..pos])
            {
                req.headers_mut().insert(http::header::USER_AGENT, val);
            }
        }
        let fut = self.inner.call(req);
        Box::pin(async move { fut.await.map_err(Into::into) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::sync::Arc;

    /// A mock service that captures the last seen User-Agent.
    #[derive(Clone, Default)]
    struct CaptureSvc {
        last_ua: Arc<std::sync::Mutex<Option<String>>>,
    }

    impl CaptureSvc {
        fn last_ua(&self) -> Option<String> {
            self.last_ua.lock().unwrap().clone()
        }
    }

    impl Service<http::Request<Body>> for CaptureSvc {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            *self.last_ua.lock().unwrap() = req
                .headers()
                .get(http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok().map(|s| s.to_string()));
            std::future::ready(Ok(http::Response::builder().status(200).body(Body::empty()).unwrap()))
        }
    }

    fn make_req(ua: Option<&str>) -> http::Request<Body> {
        let mut req = http::Request::new(Body::empty());
        if let Some(val) = ua {
            req.headers_mut()
                .insert(http::header::USER_AGENT, HeaderValue::from_str(val).unwrap());
        }
        req
    }

    #[tokio::test]
    async fn strips_tonic_suffix() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(Some("my-app/1.0 tonic/0.14.6"))).await.unwrap();
        assert_eq!(inner.last_ua().as_deref(), Some("my-app/1.0"));
    }

    #[tokio::test]
    async fn leaves_default_tonic_ua() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(Some("tonic/0.14.6"))).await.unwrap();
        assert_eq!(inner.last_ua().as_deref(), Some("tonic/0.14.6"));
    }

    #[tokio::test]
    async fn leaves_no_ua() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(None)).await.unwrap();
        assert_eq!(inner.last_ua(), None);
    }

    #[tokio::test]
    async fn preserves_intentional_tonic() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(Some("my-app tonic/0.14.6 tonic/0.14.6"))).await.unwrap();
        assert_eq!(inner.last_ua().as_deref(), Some("my-app tonic/0.14.6"));
    }

    #[tokio::test]
    async fn strips_grpctestify_default() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(Some("grpctestify/1.8.6 tonic/0.14.6"))).await.unwrap();
        assert_eq!(inner.last_ua().as_deref(), Some("grpctestify/1.8.6"));
    }

    #[tokio::test]
    async fn different_tonic_version_suffix() {
        let inner = CaptureSvc::default();
        let mut svc = StripTonicUA::new(inner.clone());
        svc.call(make_req(Some("my-app/2.0 tonic/0.15.0"))).await.unwrap();
        assert_eq!(inner.last_ua().as_deref(), Some("my-app/2.0"));
    }
}
