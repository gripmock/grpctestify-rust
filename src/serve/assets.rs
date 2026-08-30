use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

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
        Response::builder()
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
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "invalid response").into_response())
        }
        None => match Assets::get("index.html") {
            Some(content) => {
                let mime = mime_guess::from_path("index.html").first_or_octet_stream();
                Response::builder()
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
}
