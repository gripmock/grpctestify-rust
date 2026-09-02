use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

pub const STATUS_HEADER: &str = ":status";
pub const URL_HEADER: &str = ":url";
pub const FOLLOW_REDIRECTS_OPTION: &str = "follow_redirects";

pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout: Duration,
    pub follow_redirects: bool,
}

pub struct HttpAnswer {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub raw_body: String,
    pub duration_ms: u64,
}

static STAYING_CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
});

static FOLLOWING_CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())
});

fn client_for(follow_redirects: bool) -> Result<&'static reqwest::Client, String> {
    let built = if follow_redirects {
        &*FOLLOWING_CLIENT
    } else {
        &*STAYING_CLIENT
    };
    built.as_ref().map_err(Clone::clone)
}

pub fn parse_follow_redirects(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn url_for(address: Option<&str>, path: &str) -> String {
    let path = path.trim();
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let base = address.unwrap_or("").trim().trim_end_matches('/');
    if base.is_empty() {
        return path.to_string();
    }
    let base = if base.contains("://") {
        base.to_string()
    } else {
        format!("http://{base}")
    };
    if path.is_empty() {
        base
    } else if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

pub fn body_of(text: &str) -> Value {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| Value::String(text.to_string()))
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(k, _)| k.trim().eq_ignore_ascii_case(name))
}

fn is_form_field(segment: &str) -> bool {
    let Some((key, value)) = segment.split_once('=') else {
        return false;
    };
    !key.is_empty()
        && !value.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '[' | ']'))
}

pub fn content_type_of(body: &str) -> &'static str {
    let trimmed = body.trim();
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        return "application/json";
    }
    if trimmed.starts_with("<?xml") || trimmed.starts_with('<') {
        return "application/xml";
    }
    if !trimmed.is_empty()
        && trimmed.lines().count() == 1
        && trimmed.contains('=')
        && !trimmed.contains(' ')
        && trimmed.split('&').all(is_form_field)
    {
        return "application/x-www-form-urlencoded";
    }
    "text/plain"
}

pub async fn send(call: HttpCall) -> Result<HttpAnswer, String> {
    let method = reqwest::Method::from_bytes(call.method.as_bytes())
        .map_err(|_| format!("{} is not a usable HTTP method", call.method))?;

    if !call.url.contains("://") {
        return Err(format!(
            "no address for {}: an HTTP call needs a target with a scheme — this file's ADDRESS, or the environment's written as `http://host:port`",
            call.url
        ));
    }

    if let Some(scheme) = call.url.split("://").next()
        && scheme != call.url
        && scheme != "http"
        && scheme != "https"
    {
        return Err(format!(
            "{scheme}:// is not a scheme this transport dials — an HTTP test goes over http:// or https://"
        ));
    }

    let client = client_for(call.follow_redirects)?;

    let mut request = client.request(method, &call.url).timeout(call.timeout);
    for (name, value) in &call.headers {
        if name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        let header = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("{name:?} is not a usable header name"))?;
        let header_value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("the value of {name} is not one a header can carry"))?;
        request = request.header(header, header_value);
    }
    if let Some(body) = call.body {
        if !has_header(&call.headers, "content-type") {
            request = request.header("content-type", content_type_of(&body));
        }
        request = request.body(body);
    }

    let started = std::time::Instant::now();
    let response = request.send().await.map_err(|e| describe(&call.url, e))?;
    let status = response.status().as_u16();

    let mut headers: HashMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
        .collect();
    headers.insert(STATUS_HEADER.to_string(), status.to_string());
    let landed = response.url().as_str().to_string();
    if landed != call.url {
        headers.insert(URL_HEADER.to_string(), landed);
    }

    let text = response.text().await.map_err(|e| e.to_string())?;
    Ok(HttpAnswer {
        status,
        headers,
        body: body_of(&text),
        raw_body: text,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn describe(url: &str, error: reqwest::Error) -> String {
    if error.is_timeout() {
        return format!("{url} did not answer in time");
    }
    let cause = deepest_cause(&error);
    match (error.is_connect(), cause) {
        (true, Some(cause)) => format!("Could not reach {url}: {cause}"),
        (true, None) => format!("Could not reach {url}"),
        (false, Some(cause)) => format!("{url} did not answer: {cause}"),
        (false, None) => error.to_string(),
    }
}

fn deepest_cause(error: &dyn std::error::Error) -> Option<String> {
    let mut cause = error.source()?;
    while let Some(next) = cause.source() {
        cause = next;
    }
    Some(cause.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(url: String) -> HttpCall {
        HttpCall {
            method: "GET".to_string(),
            url,
            headers: Vec::new(),
            body: None,
            timeout: std::time::Duration::from_secs(5),
            follow_redirects: false,
        }
    }

    async fn serve_once(
        response: &'static [u8],
    ) -> (String, std::sync::Arc<tokio::sync::Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        let recorder = seen.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
                .await
                .unwrap_or(0);
            *recorder.lock().await = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response).await;
        });
        (format!("http://{addr}"), seen)
    }

    #[test]
    fn an_address_keeps_the_scheme_it_names() {
        assert_eq!(url_for(Some("ftp://host"), "/a"), "ftp://host/a");
        assert_eq!(url_for(Some("host:9000"), "/a"), "http://host:9000/a");
    }

    #[tokio::test]
    async fn a_header_that_cannot_be_built_is_named() {
        let mut request = call("http://127.0.0.1:1/a".to_string());
        request.headers.push((String::new(), "x".to_string()));
        let err = send(request)
            .await
            .err()
            .expect("an empty header name is refused");
        assert!(err.contains("is not a usable header name"), "{err}");
    }

    #[tokio::test]
    async fn a_hand_written_content_length_is_not_sent() {
        let (origin, seen) = serve_once(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}").await;
        let mut request = call(format!("{origin}/x"));
        request
            .headers
            .push(("content-length".to_string(), "999".to_string()));
        request
            .headers
            .push(("x-mine".to_string(), "ok".to_string()));
        let _ = send(request).await;

        let request = seen.lock().await.clone();
        assert!(
            request.contains("x-mine: ok"),
            "the rest of the headers travel: {request}"
        );
        assert!(
            !request.to_lowercase().contains("content-length: 999"),
            "the length the file named is not sent: {request}"
        );
    }

    #[tokio::test]
    async fn headers_travel_in_the_order_the_file_wrote_them() {
        let (origin, seen) = serve_once(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}").await;
        let mut request = call(format!("{origin}/x"));
        for name in [
            "x-zulu",
            "x-alpha",
            "x-mike",
            "x-bravo",
            "x-yankee",
            "x-charlie",
        ] {
            request.headers.push((name.to_string(), "1".to_string()));
        }
        let _ = send(request).await;

        let request = seen.lock().await.clone();
        let positions: Vec<usize> = [
            "x-zulu",
            "x-alpha",
            "x-mike",
            "x-bravo",
            "x-yankee",
            "x-charlie",
        ]
        .iter()
        .map(|name| {
            request
                .find(name)
                .unwrap_or_else(|| panic!("{name}: {request}"))
        })
        .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "headers left in file order: {request}"
        );
    }

    #[tokio::test]
    async fn a_redirect_is_the_answer_unless_the_call_asks_to_follow() {
        let (origin, _) =
            serve_once(b"HTTP/1.1 302 Found\r\nlocation: /elsewhere\r\ncontent-length: 0\r\n\r\n")
                .await;
        let answer = send(call(format!("{origin}/start")))
            .await
            .expect("the 302 is an answer");
        assert_eq!(answer.status, 302);
        assert_eq!(
            answer.headers.get("location").map(String::as_str),
            Some("/elsewhere")
        );
        assert!(!answer.headers.contains_key(URL_HEADER));
    }

    #[tokio::test]
    async fn following_a_redirect_lands_where_it_points() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
                    .await
                    .unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let response: &[u8] = if request.starts_with("GET /start") {
                    b"HTTP/1.1 302 Found\r\nlocation: /landed\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                } else {
                    b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\nconnection: close\r\n\r\n{\"here\": true}"
                };
                let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response).await;
            }
        });

        let mut request = call(format!("http://{addr}/start"));
        request.follow_redirects = true;
        let answer = send(request).await.expect("followed");
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, serde_json::json!({"here": true}));
        assert_eq!(
            answer.headers.get(URL_HEADER).map(String::as_str),
            Some(format!("http://{addr}/landed").as_str())
        );
    }

    #[test]
    fn the_follow_redirects_option_reads_like_the_other_booleans() {
        assert_eq!(parse_follow_redirects("true"), Some(true));
        assert_eq!(parse_follow_redirects(" Yes "), Some(true));
        assert_eq!(parse_follow_redirects("0"), Some(false));
        assert_eq!(parse_follow_redirects("sometimes"), None);
    }

    #[tokio::test]
    async fn a_scheme_this_transport_cannot_dial_says_so() {
        let err = send(call("ftp://host/a".to_string()))
            .await
            .err()
            .expect("ftp is not dialled");
        assert!(err.contains("ftp:// is not a scheme"), "{err}");
    }

    #[test]
    fn an_absolute_url_is_used_as_written() {
        assert_eq!(
            url_for(Some("localhost:9000"), "https://api.example.com/v1/users"),
            "https://api.example.com/v1/users"
        );
    }

    #[test]
    fn a_path_is_joined_to_the_address() {
        assert_eq!(
            url_for(Some("https://api.example.com"), "/v1/users"),
            "https://api.example.com/v1/users"
        );
        assert_eq!(
            url_for(Some("https://api.example.com/"), "v1/users"),
            "https://api.example.com/v1/users"
        );
    }

    #[test]
    fn an_address_without_a_scheme_is_http() {
        assert_eq!(
            url_for(Some("localhost:8080"), "/health"),
            "http://localhost:8080/health"
        );
    }

    #[test]
    fn a_path_alone_stays_a_path() {
        assert_eq!(url_for(None, "/v1/users"), "/v1/users");
    }

    #[test]
    fn a_body_says_what_it_is_when_the_file_does_not() {
        assert_eq!(content_type_of("{\"a\": 1}"), "application/json");
        assert_eq!(content_type_of("[1, 2]"), "application/json");
        assert_eq!(
            content_type_of("name=Ada&age=36"),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            content_type_of("<?xml version=\"1.0\"?><a/>"),
            "application/xml"
        );
        assert_eq!(content_type_of("<html></html>"), "application/xml");
        assert_eq!(content_type_of("just some words"), "text/plain");
    }

    #[test]
    fn a_bare_scalar_is_json_when_it_parses_as_one() {
        assert_eq!(content_type_of("123"), "application/json");
        assert_eq!(content_type_of("true"), "application/json");
        assert_eq!(content_type_of("null"), "application/json");
        assert_eq!(content_type_of("\"quoted\""), "application/json");
        assert_eq!(content_type_of("2024-06-15"), "text/plain");
        assert_eq!(content_type_of("hello"), "text/plain");
    }

    #[test]
    fn a_form_value_may_carry_its_own_equals_sign() {
        assert_eq!(
            content_type_of("token=aGVsbG8="),
            "application/x-www-form-urlencoded",
            "base64 padding is a form value, not prose"
        );
        assert_eq!(
            content_type_of("a=1&sig=YWJj==&b=2"),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            content_type_of("q=rust&page[size]=10"),
            "application/x-www-form-urlencoded"
        );
    }

    #[test]
    fn a_stray_equals_sign_is_not_a_form() {
        assert_eq!(content_type_of("abc="), "text/plain");
        assert_eq!(content_type_of("=abc"), "text/plain");
        assert_eq!(content_type_of("a=b&nonsense"), "text/plain");
    }

    #[test]
    fn a_body_that_is_json_is_json_and_the_rest_is_text() {
        assert_eq!(body_of(" {\"a\": 1} "), serde_json::json!({"a": 1}));
        assert_eq!(body_of("hello"), Value::String("hello".to_string()));
        assert_eq!(body_of("   "), Value::Null);
    }
}

#[cfg(test)]
mod no_target {
    use super::*;

    #[tokio::test]
    async fn a_call_with_no_address_says_what_is_missing() {
        let error = send(HttpCall {
            method: "GET".to_string(),
            url: "/v1/users".to_string(),
            headers: Vec::new(),
            body: None,
            timeout: std::time::Duration::from_secs(1),
            follow_redirects: false,
        })
        .await
        .err()
        .expect("refused");
        assert!(error.contains("no address for /v1/users"), "{error}");
        assert!(error.contains("ADDRESS"), "{error}");
    }
}
