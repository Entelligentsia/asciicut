//! HTTP handlers for the `/api/*` surface plus the SPA fallback.
//!
//! Every in-scope endpoint (`frame`/`thumbs`/`activity`/`compose`/`save`/
//! `project`) is a thin wrapper over the matching `asciicut_bridge::ops`
//! function — a few lines to unwrap the `State<Arc<AppState>>`, call the
//! bridge, and either wrap the `Ok` DTO in `Json` or map a
//! [`asciicut_bridge::BridgeError`] to the same [`ApiError`] (status code,
//! message) these handlers returned before the extraction (CASTCU-S2-T02), so
//! HTTP status-code behavior (400 for a bad project, 422 for a corrupt
//! persisted project, 500 for I/O failure) is unchanged. `/api/export` is the
//! one handler with no bridge endpoint of its own (see `asciicut-bridge`'s
//! crate docs); it composes two bridge ops plus a server-derived write target.
//! The shared [`AppState`] (bridge `Session` plus, for `export`, the
//! server-derived export path) is read-mostly and `Arc`-shared across
//! requests.

use std::sync::Arc;

use asciicut_bridge::{dto, ops, BridgeError};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::assets;
use crate::AppState;

/// A structured API error rendered as `(status, plain-text message)`.
pub(crate) struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

/// Map a bridge failure to the exact HTTP status contract the handlers had
/// before the `asciicut-bridge` extraction: `InvalidProject` → `400`,
/// `InvalidPersistedProject` → `422`, everything else → `500`.
/// `NoCastLoaded`/`ReadCast`/`ParseCast` are not reachable through these
/// handlers (server `AppState` always has a cast loaded — see its struct-level
/// invariant note) but are mapped defensively rather than asserted unreachable.
impl From<BridgeError> for ApiError {
    fn from(err: BridgeError) -> Self {
        match err {
            BridgeError::InvalidProject(e) => {
                ApiError(StatusCode::BAD_REQUEST, format!("invalid project: {e}"))
            }
            BridgeError::InvalidPersistedProject(msg) => ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("persisted project is invalid: {msg}"),
            ),
            BridgeError::Io { path, source } => ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot write {}: {source}", path.display()),
            ),
            BridgeError::NoCastLoaded => {
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, "no cast loaded".into())
            }
            BridgeError::ReadCast { path, source } => ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot read {}: {source}", path.display()),
            ),
            BridgeError::ParseCast(e) => ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid source cast: {e}"),
            ),
        }
    }
}

/// `GET /api/frame?t=<seconds>` — the styled screen grid at source time `T`.
///
/// A missing or non-numeric `t` fails query deserialization and axum returns
/// `400` automatically.
pub async fn frame(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FrameQuery>,
) -> Json<dto::FrameDto> {
    // `ops::frame` only fails with `NoCastLoaded`, which cannot happen here —
    // server `AppState` always has a cast loaded (its struct-level invariant).
    let frame_dto =
        ops::frame(state.session(), query.t).expect("server AppState always has a loaded cast");
    Json(frame_dto)
}

/// Query for [`frame`].
#[derive(Debug, Deserialize)]
pub struct FrameQuery {
    /// Source time in seconds. Required — its absence is a `400`.
    t: f64,
}

/// `GET /api/thumbs?count=<n>` — `n` evenly-spaced filmstrip frames over the
/// recording's duration.
///
/// `count` defaults to the bridge's `DEFAULT_THUMBS` and is clamped to
/// `1..=MAX_THUMBS` (both enforced inside `asciicut_bridge::ops::thumbs`). Each
/// element carries the sampled source time and its frame DTO; the SPA (T09)
/// draws text-cell thumbnails on canvas. PNG rendering via `agg` is a deferred
/// follow-up (`/render`).
pub async fn thumbs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ThumbsQuery>,
) -> Json<Vec<dto::ThumbDto>> {
    let thumbs = ops::thumbs(state.session(), query.count)
        .expect("server AppState always has a loaded cast");
    Json(thumbs)
}

/// Query for [`thumbs`].
#[derive(Debug, Deserialize)]
pub struct ThumbsQuery {
    /// Number of frames to sample. Defaults to the bridge's `DEFAULT_THUMBS`,
    /// clamped to `1..=MAX_THUMBS`.
    count: Option<usize>,
}

/// `GET /api/activity?bucket=<seconds>` — the change-density waveform of the
/// launch cast, one score per time bucket.
///
/// `bucket` defaults to `DEFAULT_BUCKET_SECS` and is clamped up to the
/// bridge's `MIN_BUCKET_SECS` floor for any finite-positive value (see
/// `asciicut_bridge::ops::activity`'s docs for the crash-guard rationale).
pub async fn activity(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ActivityQuery>,
) -> Json<dto::ActivitySignalDto> {
    let signal = ops::activity(state.session(), query.bucket)
        .expect("server AppState always has a loaded cast");
    Json(signal)
}

/// Query for [`activity`].
#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    /// Bucket duration in seconds. Defaults to `DEFAULT_BUCKET_SECS`; a
    /// finite-positive value is clamped up to the bridge's `MIN_BUCKET_SECS`.
    bucket: Option<f64>,
}

/// `GET /api/events` — the launch cast's real event timestamps, ascending and
/// de-duplicated.
///
/// The SPA snaps IN/OUT nudges to these, so a nudged boundary always lands on a
/// frame where something actually changed rather than on an arbitrary step.
pub async fn events(State(state): State<Arc<AppState>>) -> Json<Vec<f64>> {
    let times =
        ops::event_times(state.session()).expect("server AppState always has a loaded cast");
    Json(times)
}

/// `POST /api/compose` — compose the request-body edit project against the
/// launch-time source cast and return the composed `.cast` document.
///
/// The body is a `.asciicut.json` project. A parse/validation failure is a `400`.
/// The composed document is returned as `text/plain` (it is JSON *Lines*, not a
/// single JSON value).
pub async fn compose_project(
    State(state): State<Arc<AppState>>,
    body: String,
) -> Result<Response, ApiError> {
    let document = ops::compose_project(state.session(), &body)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        document,
    )
        .into_response())
}

/// `POST /api/save` — persist the request-body edit project to a
/// **server-derived** path (`<cast_dir>/<cast_stem>.asciicut.json`).
///
/// The target path is derived from the launch cast path and is **never** taken
/// from the request (path-traversal surface, even on localhost — advisory #1).
/// The body is validated before it is written, so malformed input is rejected
/// with `400` rather than persisted.
pub async fn save(
    State(state): State<Arc<AppState>>,
    body: String,
) -> Result<Json<dto::SaveResponse>, ApiError> {
    let response = ops::save_project(state.session(), &body)?;
    Ok(Json(response))
}

/// `GET /api/project` — the launch cast's authoritative `source` filename plus
/// any persisted `.asciicut.json` (for load-on-open, AC#1).
///
/// The persisted file (if present at the **server-derived** save path) is
/// validated and then echoed as a raw [`serde_json::Value`] — so **no
/// `Serialize` derive is added to `asciicut-core`** and the native↔wasm parity
/// boundary (SPEC §7.1) stays untouched. A missing file yields `project: null`;
/// a file that exists but fails validation surfaces a `422` (it is NOT
/// silently reported as absent — review advisory).
pub async fn project(
    State(state): State<Arc<AppState>>,
) -> Result<Json<dto::ProjectMeta>, ApiError> {
    let meta = ops::project_meta(state.session())?;
    Ok(Json(meta))
}

/// `POST /api/export` — validate the request-body edit project, persist it to
/// the server-derived `.asciicut.json`, then compose it against the launch cast
/// and write the composed `<stem>.composed.cast` (AC#2). Both targets are
/// **server-derived** (never request input, advisory #1).
///
/// Not a bridge *endpoint* (CASTCU-S2-T02 scope) — the desktop export story is
/// a different `agg`/`ffmpeg`-backed pipeline, not a bridge of this
/// JSON-project-writer — but its two steps are the bridge's own
/// `save_project` + `compose_project` ops, so validation, the save target, and
/// the error→status mapping are shared with `/api/save` and `/api/compose`
/// rather than re-implemented here.
///
/// Byte-identity (AC#3): the composed document funnels through the SAME
/// `compose(&cast, &project).to_cast_string()` serializer as `/api/compose` and
/// the `asciicut compose` CLI, so the exported `.cast` equals the in-browser
/// preview for the same project. A parse/validation failure is a `400`; an
/// on-disk write failure is a `500`. Both writes OVERWRITE any existing target.
pub async fn export(
    State(state): State<Arc<AppState>>,
    body: String,
) -> Result<Json<ExportResponse>, ApiError> {
    // Validate + persist the project through the same bridge op `/api/save`
    // uses, so the two endpoints cannot disagree on what a valid project is,
    // where it lands, or which status a failure maps to.
    let saved = ops::save_project(state.session(), &body)?;
    let document = ops::compose_project(state.session(), &body)?;

    let cast_path = state.export_cast_path();
    std::fs::write(&cast_path, &document).map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot write {}: {e}", cast_path.display()),
        )
    })?;

    Ok(Json(ExportResponse {
        project_path: saved.path,
        cast_path: cast_path.display().to_string(),
        bytes: document.len(),
    }))
}

/// Response for [`export`]: the two server-derived paths written and the
/// composed byte count. camelCase to match the SPA DTO mirror.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResponse {
    /// The path the `.asciicut.json` project was written to.
    project_path: String,
    /// The path the composed `.cast` was written to.
    cast_path: String,
    /// Byte length of the composed `.cast` document.
    bytes: usize,
}

/// Fallback handler for every non-`/api` path: serve the embedded SPA, with
/// `index.html` covering client-side routes.
pub async fn spa_fallback(uri: Uri) -> Response {
    assets::serve_asset(&uri)
}
