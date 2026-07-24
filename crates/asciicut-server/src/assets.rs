//! Embedded SPA assets — the SolidJS web UI baked into the binary via
//! `rust-embed` (SPEC §7.2, AC#1).
//!
//! The asset folder is `web/`. It carries a committed placeholder `index.html`
//! today; CASTCU-S1-T08's Vite build output replaces it. `web/` (not `dist/`) is
//! the folder because `.gitignore` ignores `dist/`, and rust-embed requires the
//! folder to exist at compile time. The `debug-embed` feature (set in
//! `Cargo.toml`) makes rust-embed embed the bytes in debug builds too, so
//! `cargo test` behaves identically to a release build.

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// The embedded SPA asset store. Every file under `web/` at compile time is baked
/// into the binary.
#[derive(RustEmbed)]
#[folder = "web/"]
struct Assets;

/// Serve an embedded asset by request path, falling back to `index.html` for any
/// path that does not name a baked-in file.
///
/// This is the SPA fallback: unknown non-`/api` paths (client-side routes, deep
/// links) resolve to `index.html` so the SPA router owns them. A `Content-Type`
/// is inferred from the served path's extension via `mime_guess`. Returns `404`
/// only when even `index.html` is absent (a broken build).
pub fn serve_asset(uri: &Uri) -> Response {
    // Strip the leading '/'; an empty path means the SPA root.
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match serve_path(path) {
        Some(response) => response,
        // SPA fallback: hand any unmatched path to index.html so the client
        // router can resolve it. `index.html` is always present (committed).
        None => match serve_path("index.html") {
            Some(response) => response,
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

/// Look up one exact embedded file and wrap it in a `Response` with a guessed
/// content type. `None` when the path names no baked-in asset.
fn serve_path(path: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(asset.data.into_owned()))
            .expect("static asset response is always well-formed"),
    )
}
