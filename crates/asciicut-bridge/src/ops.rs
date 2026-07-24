//! One plain-Rust operation per data call. Each function is a thin wrapper
//! over a pure `asciicut-core` primitive plus [`Session`] state — the *only*
//! implementation either transport (axum handler or Tauri command) calls
//! (AC#1). No `axum` types (`Json`/`Query`/`State`), no HTTP status codes.

use asciicut_core::{activity_signal, compose, frame_at, Project, DEFAULT_BUCKET_SECS};

use crate::dto::{ActivitySignalDto, FrameDto, ProjectMeta, SaveResponse, ThumbDto};
use crate::error::BridgeError;
use crate::session::Session;

/// Default filmstrip sample count when the caller omits `count`.
const DEFAULT_THUMBS: usize = 12;

/// Hard cap on `thumbs`' `count`. Each sample is a full `frame_at` replay
/// (O(events) per frame), so an unbounded `count` is O(N·events) CPU per call;
/// the cap bounds the worst case. Relocated verbatim from the axum handler
/// (`asciicut-server::routes::MAX_THUMBS`) — this guard MUST live here so the
/// desktop transport does not lose the abort-guard the server had (review
/// advisory).
const MAX_THUMBS: usize = 240;

/// Floor on `activity`'s `bucket`. The core's `activity_signal` rejects only
/// non-finite/non-positive bucket sizes; any *tiny positive* value (e.g.
/// `1e-7`) passes its guard, saturates the derived bucket count toward
/// `usize::MAX`, and `vec![0u64; count]` aborts the process — a crash from a
/// single call. Clamping the incoming bucket up to this floor before calling
/// the core bounds the allocation. Relocated verbatim from the axum handler
/// (`asciicut-server::routes::MIN_BUCKET_SECS`) — same rationale as
/// `MAX_THUMBS` above: this guard must live here, not only in the (removed)
/// server-side duplicate, or the desktop transport loses it.
const MIN_BUCKET_SECS: f64 = 0.05;

/// The styled screen grid at source time `t`.
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded.
pub fn frame(session: &Session, t: f64) -> Result<FrameDto, BridgeError> {
    let cast = session.cast()?;
    let frame = frame_at(cast, t);
    Ok(FrameDto::from(&frame))
}

/// `count` evenly-spaced filmstrip frames over the recording's duration.
///
/// `count` defaults to [`DEFAULT_THUMBS`] and is clamped to `1..=MAX_THUMBS`.
/// Each element carries the sampled source time and its frame DTO.
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded.
pub fn thumbs(session: &Session, count: Option<usize>) -> Result<Vec<ThumbDto>, BridgeError> {
    let cast = session.cast()?;
    let count = count.unwrap_or(DEFAULT_THUMBS).clamp(1, MAX_THUMBS);
    let duration = cast.events.last().map_or(0.0, |event| event.time);

    let mut thumbs = Vec::with_capacity(count);
    for i in 0..count {
        // Evenly spaced including both endpoints; a single sample is the start.
        let t = if count == 1 {
            0.0
        } else {
            duration * (i as f64) / ((count - 1) as f64)
        };
        let frame = frame_at(cast, t);
        thumbs.push(ThumbDto {
            t,
            frame: FrameDto::from(&frame),
        });
    }
    Ok(thumbs)
}

/// The change-density waveform of the loaded cast, one score per time bucket.
///
/// `bucket` defaults to `DEFAULT_BUCKET_SECS`. A **finite-positive** `bucket`
/// is clamped *up* to [`MIN_BUCKET_SECS`] before the core call so a tiny value
/// cannot saturate the bucket-count allocation and crash the process. A
/// non-finite / non-positive `bucket` is passed through unchanged so the
/// core's own guard performs its documented fallback to `DEFAULT_BUCKET_SECS`
/// (a naive `f64::max` floor would instead pin `NaN`/negative to the floor,
/// which would break that fallback).
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded.
pub fn activity(session: &Session, bucket: Option<f64>) -> Result<ActivitySignalDto, BridgeError> {
    let cast = session.cast()?;
    let requested = bucket.unwrap_or(DEFAULT_BUCKET_SECS);
    let bucket = if requested.is_finite() && requested > 0.0 {
        requested.max(MIN_BUCKET_SECS)
    } else {
        // Leave the fallback to the core guard (→ DEFAULT_BUCKET_SECS).
        requested
    };
    let signal = activity_signal(cast, bucket);
    Ok(ActivitySignalDto::from(&signal))
}

/// The loaded cast's **real event timestamps**, ascending and de-duplicated.
///
/// This is the honest quantum the editor's IN/OUT nudge snaps to: an in-point
/// nudged onto one of these always lands on a frame where something actually
/// changed, rather than on an arbitrary fixed step or an activity-bucket
/// boundary (which only says "something changed *within* this bucket").
///
/// A `.cast`'s events are already stored in ascending time order, but several
/// events may share a timestamp (a burst written in one tick); collapsing those
/// keeps the returned list a set of distinct *moments*, so a single nudge never
/// appears to do nothing.
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded.
pub fn event_times(session: &Session) -> Result<Vec<f64>, BridgeError> {
    let cast = session.cast()?;
    let mut times = Vec::with_capacity(cast.events.len());
    for event in &cast.events {
        // Ascending-by-construction, so a trailing-duplicate check is enough.
        if times.last().is_none_or(|last: &f64| *last < event.time) {
            times.push(event.time);
        }
    }
    Ok(times)
}

/// Compose `project_body` against the loaded source cast and return the
/// composed `.cast` document.
///
/// `project_body` is a `.asciicut.json` project; a parse/validation failure is
/// [`BridgeError::InvalidProject`]. The composed document funnels through the
/// exact same `compose(&cast, &project).to_cast_string()` call the CLI's
/// `asciicut::compose_project` uses (AC#5 byte-identity).
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded, or
/// [`BridgeError::InvalidProject`] if `project_body` fails to parse.
pub fn compose_project(session: &Session, project_body: &str) -> Result<String, BridgeError> {
    let cast = session.cast()?;
    let project = Project::parse(project_body).map_err(BridgeError::InvalidProject)?;
    let composed = compose(cast, &project);
    Ok(composed.to_cast_string())
}

/// Persist `project_body` to the session-derived path
/// (`<cast_dir>/<cast_stem>.asciicut.json`) — never a caller-supplied path
/// (path-traversal surface, even in-process).
///
/// The body is validated with `Project::parse` before it is written, so
/// malformed input is rejected rather than persisted.
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded,
/// [`BridgeError::InvalidProject`] if `project_body` fails to parse, or
/// [`BridgeError::Io`] if the write fails.
pub fn save_project(session: &Session, project_body: &str) -> Result<SaveResponse, BridgeError> {
    save_project_to(session, project_body, session.save_path()?)
}

/// Persist `project_body` to an explicit, OS-dialog-derived path. This is the
/// Save As implementation: the caller is the desktop Rust command that just
/// obtained the path from the native file picker, so provenance stays inside
/// the shell. The body is validated before writing.
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded,
/// [`BridgeError::InvalidProject`] if `project_body` fails to parse, or
/// [`BridgeError::Io`] if the write fails.
pub fn save_project_to(
    session: &Session,
    project_body: &str,
    path: std::path::PathBuf,
) -> Result<SaveResponse, BridgeError> {
    let _cast = session.cast()?; // ensure a cast is loaded
    Project::parse(project_body).map_err(BridgeError::InvalidProject)?;

    std::fs::write(&path, project_body).map_err(|source| BridgeError::Io {
        path: path.clone(),
        source,
    })?;

    Ok(SaveResponse {
        path: path.display().to_string(),
        bytes: project_body.len(),
    })
}

/// The loaded cast's authoritative `source` filename plus any persisted
/// `.asciicut.json` at the session-derived save path (for load-on-open).
///
/// The persisted file (if present) is validated with `Project::parse` and
/// then echoed as a raw [`serde_json::Value`] — so no `Serialize` derive is
/// added to `asciicut-core` and the native↔wasm parity boundary stays
/// untouched. A missing file yields `project: None`; a file that exists but
/// fails validation surfaces [`BridgeError::InvalidPersistedProject`] (it is
/// NOT silently reported as absent).
///
/// # Errors
///
/// Returns [`BridgeError::NoCastLoaded`] if `session` has no cast loaded,
/// [`BridgeError::InvalidPersistedProject`] if a persisted file exists but
/// fails to parse/validate, or [`BridgeError::Io`] if reading it fails for any
/// other reason.
pub fn project_meta(session: &Session) -> Result<ProjectMeta, BridgeError> {
    let path = session.save_path()?;
    let project = match std::fs::read_to_string(&path) {
        Ok(text) => {
            // Validate before echoing so a corrupt persisted file is a loud
            // error, never a silent None that would mask a broken save.
            Project::parse(&text)
                .map_err(|e| BridgeError::InvalidPersistedProject(e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| BridgeError::InvalidPersistedProject(e.to_string()))?;
            Some(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(BridgeError::Io {
                path: path.clone(),
                source,
            });
        }
    };
    Ok(ProjectMeta {
        source: session.source_name()?,
        project,
    })
}
