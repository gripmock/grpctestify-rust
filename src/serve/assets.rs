use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

static CONTENT_SECURITY_POLICY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let hashes = Assets::get("index.html")
        .and_then(|index| String::from_utf8(index.data.to_vec()).ok())
        .map(|html| inline_script_hashes(&html))
        .unwrap_or_default();
    content_security_policy(&hashes)
});

fn inline_script_hashes(html: &str) -> Vec<String> {
    use sha2::Digest;
    let mut hashes = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<script") {
        let tag = &rest[at..];
        let Some(open_end) = tag.find('>') else {
            break;
        };
        let attrs = &tag[..open_end];
        let body_and_beyond = &tag[open_end + 1..];
        let Some(close) = body_and_beyond.find("</script>") else {
            break;
        };
        if !attrs.contains("src=") {
            let digest = sha2::Sha256::digest(&body_and_beyond.as_bytes()[..close]);
            hashes.push(format!("sha256-{}", base64(&digest)));
        }
        rest = &body_and_beyond[close..];
    }
    hashes
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                let index = ((word >> (18 - 6 * i)) & 0x3f) as usize;
                out.push(char::from(TABLE[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn content_security_policy(script_hashes: &[String]) -> String {
    let scripts = std::iter::once("'self'".to_string())
        .chain(script_hashes.iter().map(|h| format!("'{h}'")))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "default-src 'self'; script-src {scripts}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; worker-src 'self' blob:; \
         connect-src 'self'; frame-ancestors 'none'"
    )
}

fn secured(builder: axum::http::response::Builder) -> axum::http::response::Builder {
    builder
        .header(
            header::CONTENT_SECURITY_POLICY,
            CONTENT_SECURITY_POLICY.as_str(),
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
}

pub fn build_id() -> Option<String> {
    let index = Assets::get("index.html")?;
    entry_chunk(std::str::from_utf8(&index.data).ok()?)
}

fn entry_chunk(html: &str) -> Option<String> {
    let at = html.find("/assets/index-")?;
    let rest = &html[at..];
    let end = rest.find('"')?;
    Some(rest[..end].trim_start_matches('/').to_string())
}

pub fn try_get_asset(path: &str) -> Option<Response> {
    let filename = path.trim_start_matches('/');
    if filename.is_empty() {
        return None;
    }
    Assets::get(filename).map(|content| {
        let mime = mime_guess::from_path(filename).first_or_octet_stream();
        secured(Response::builder())
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data))
            .unwrap_or_else(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, "invalid response").into_response()
            })
    })
}

pub async fn handle_embedded(path: &str) -> Response {
    let filename = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    match Assets::get(filename) {
        Some(content) => {
            let mime = mime_guess::from_path(filename).first_or_octet_stream();
            secured(Response::builder())
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "invalid response").into_response())
        }
        None => match Assets::get("index.html") {
            Some(content) => {
                let mime = mime_guess::from_path("index.html").first_or_octet_stream();
                secured(Response::builder())
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(Body::from(content.data))
                    .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "invalid response").into_response())
            }
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_chunk_is_the_hashed_one() {
        let html = r#"<!doctype html><script type="module" crossorigin src="/assets/index-0EXXaJMm.js"></script>"#;
        assert_eq!(
            entry_chunk(html).as_deref(),
            Some("assets/index-0EXXaJMm.js")
        );
    }

    #[test]
    fn an_index_without_one_names_no_build() {
        assert_eq!(entry_chunk("<!doctype html><body>nothing</body>"), None);
    }

    #[test]
    fn this_binary_knows_its_build() {
        let id = build_id().expect("the embedded index names an entry chunk");
        assert!(id.starts_with("assets/index-"), "{id}");
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn inline_scripts_are_hashed_and_external_ones_are_not() {
        use sha2::Digest;
        let html = r#"<head><script>alert(1)</script><script type="module" src="/assets/index.js"></script></head>"#;
        let hashes = inline_script_hashes(html);
        let expected = format!("sha256-{}", base64(&sha2::Sha256::digest(b"alert(1)")));
        assert_eq!(hashes, vec![expected.clone()]);
        let policy = content_security_policy(&hashes);
        assert!(
            policy.contains(&format!("script-src 'self' '{expected}'")),
            "{policy}"
        );
        assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
        assert!(!policy.contains("unsafe-eval"), "{policy}");
        assert!(inline_script_hashes("<body>plain</body>").is_empty());
    }

    #[test]
    fn the_embedded_page_ships_with_a_policy_that_allows_its_own_bootstrap() {
        let index = Assets::get("index.html").expect("index");
        let html = std::str::from_utf8(&index.data).expect("utf8");
        let hashes = inline_script_hashes(html);
        let policy = CONTENT_SECURITY_POLICY.as_str();
        for hash in &hashes {
            assert!(policy.contains(hash), "{policy}");
        }
        assert!(policy.contains("worker-src 'self' blob:"), "{policy}");
    }

    #[tokio::test]
    async fn every_served_file_carries_the_security_headers() {
        for response in [
            handle_embedded("").await,
            handle_embedded("/nothing/like/this").await,
            try_get_asset("manifest.json").expect("manifest is embedded"),
        ] {
            let headers = response.headers();
            assert_eq!(
                headers
                    .get(header::CONTENT_SECURITY_POLICY)
                    .and_then(|v| v.to_str().ok()),
                Some(CONTENT_SECURITY_POLICY.as_str())
            );
            assert_eq!(
                headers
                    .get(header::X_CONTENT_TYPE_OPTIONS)
                    .and_then(|v| v.to_str().ok()),
                Some("nosniff")
            );
            assert_eq!(
                headers
                    .get(header::REFERRER_POLICY)
                    .and_then(|v| v.to_str().ok()),
                Some("no-referrer")
            );
        }
    }
}
