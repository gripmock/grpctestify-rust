use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

/// Try to serve a file from embedded assets.
/// Returns `None` if the file is not found (caller can fall back to index.html).
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

/// Serve embedded file or fall back to index.html for SPA/client-side routing.
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
        None => {
            // Try index.html fallback for SPA routes
            match Assets::get("index.html") {
                Some(content) => {
                    let mime = mime_guess::from_path("index.html").first_or_octet_stream();
                    Response::builder()
                        .header(header::CONTENT_TYPE, mime.as_ref())
                        .body(Body::from(content.data))
                        .unwrap_or_else(|_| {
                            (StatusCode::NOT_FOUND, "invalid response").into_response()
                        })
                }
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }
}
