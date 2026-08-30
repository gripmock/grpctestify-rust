use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub const STATUS_HEADER: &str = ":status";
pub const URL_HEADER: &str = ":url";

pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub timeout: Duration,
}

pub struct HttpAnswer {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub raw_body: String,
    pub duration_ms: u64,
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

fn has_header(headers: &HashMap<String, String>, name: &str) -> bool {
    headers.keys().any(|k| k.eq_ignore_ascii_case(name))
}

pub fn content_type_of(body: &str) -> &'static str {
    let trimmed = body.trim_start();
    if serde_json::from_str::<Value>(body).is_ok() {
        return "application/json";
    }
    if trimmed.starts_with("<?xml") || trimmed.starts_with('<') {
        return "application/xml";
    }
    if !trimmed.is_empty()
        && trimmed.lines().count() == 1
        && trimmed.contains('=')
        && !trimmed.contains(' ')
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

    let client = reqwest::Client::builder()
        .timeout(call.timeout)
        .build()
        .map_err(|e| e.to_string())?;

    let mut request = client.request(method, &call.url);
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

    #[test]
    fn an_address_keeps_the_scheme_it_names() {
        assert_eq!(url_for(Some("ftp://host"), "/a"), "ftp://host/a");
        assert_eq!(url_for(Some("host:9000"), "/a"), "http://host:9000/a");
    }

    #[tokio::test]
    async fn a_header_that_cannot_be_built_is_named() {
        let mut headers = HashMap::new();
        headers.insert(String::new(), "x".to_string());
        let err = send(HttpCall {
            method: "GET".to_string(),
            url: "http://127.0.0.1:1/a".to_string(),
            headers,
            body: None,
            timeout: std::time::Duration::from_secs(1),
        })
        .await
        .err()
        .expect("an empty header name is refused");
        assert!(err.contains("is not a usable header name"), "{err}");
    }

    #[tokio::test]
    async fn a_hand_written_content_length_is_not_sent() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        let recorder = seen.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let n = tokio::io::AsyncReadExt::read(&mut socket, &mut buf)
                .await
                .unwrap_or(0);
            *recorder.lock().await = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}",
            )
            .await;
        });

        let mut headers = HashMap::new();
        headers.insert("content-length".to_string(), "999".to_string());
        headers.insert("x-mine".to_string(), "ok".to_string());
        let _ = send(HttpCall {
            method: "GET".to_string(),
            url: format!("http://{addr}/x"),
            headers,
            body: None,
            timeout: std::time::Duration::from_secs(5),
        })
        .await;

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
    async fn a_scheme_this_transport_cannot_dial_says_so() {
        let err = send(HttpCall {
            method: "GET".to_string(),
            url: "ftp://host/a".to_string(),
            headers: HashMap::new(),
            body: None,
            timeout: std::time::Duration::from_secs(1),
        })
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
            headers: HashMap::new(),
            body: None,
            timeout: std::time::Duration::from_secs(1),
        })
        .await
        .err()
        .expect("refused");
        assert!(error.contains("no address for /v1/users"), "{error}");
        assert!(error.contains("ADDRESS"), "{error}");
    }
}
