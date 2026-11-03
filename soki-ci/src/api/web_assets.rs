use std::path::Path;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rust_embed::RustEmbed;

use super::ApiState;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

/// Serve the embedded SPA, or `web/dist` from disk when `--web-dir` is used.
/// `/api/*` misses stay JSON 404s so client bugs are obvious.
pub async fn spa_fallback(State(state): State<ApiState>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    if path == "/api" || path.starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        )
            .into_response();
    }

    if request.method() != Method::GET && request.method() != Method::HEAD {
        return (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response();
    }

    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let immutable = path.starts_with("/assets/");

    if let Some(response) = asset_response(&state, relative, immutable) {
        return response;
    }

    // Unknown file-like paths should 404 instead of returning HTML.
    if Path::new(relative).extension().is_some() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    if let Some(response) = asset_response(&state, "index.html", false) {
        return response;
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        "soki-ci web UI is not built. Run `pnpm --dir web install && pnpm --dir web build`, then rebuild soki-ci.",
    )
        .into_response()
}

fn asset_response(state: &ApiState, relative: &str, immutable: bool) -> Option<Response> {
    if let Some(dir) = &state.web_dir {
        let path = dir.join(relative);
        if path.is_file() {
            if let Ok(bytes) = std::fs::read(&path) {
                return Some(build_response(relative, bytes, immutable));
            }
        }
    }
    let asset = Assets::get(relative)?;
    Some(build_response(relative, asset.data.into_owned(), immutable))
}

fn build_response(relative: &str, bytes: Vec<u8>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(relative)
        .first_or_octet_stream()
        .to_string();
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .header(CACHE_CONTROL, cache)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
