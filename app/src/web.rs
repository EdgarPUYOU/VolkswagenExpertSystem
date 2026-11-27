//! Serves the built web bundle (web/dist) from inside the binary, with SPA fallback.
use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct Assets;

pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Vite fingerprints files under assets/, so they can be cached forever.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [(header::CONTENT_TYPE, mime.to_string()), (header::CACHE_CONTROL, cache.to_string())],
            file.data.into_owned(),
        )
            .into_response();
    }
    // Unknown API paths must stay 404; anything else is a client-side route.
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Assets::get("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")],
            index.data.into_owned(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "web bundle not built: run `npm run build` in web/").into_response(),
    }
}
