//! [`BridgeError`] — the bridge's single structured failure type.
//!
//! Every [`crate::ops`] function and [`crate::Session::load`] returns this
//! enum on failure. Each transport maps it to its own convention: an axum
//! handler maps a variant to an HTTP status code (preserving the existing
//! 400/422/500 contract — see `asciicut-server::routes`'s `From<BridgeError>
//! for ApiError`); a Tauri command maps it to a `Result::Err(String)`, which
//! `invoke()` surfaces to the frontend as a rejected promise.

use std::fmt;
use std::path::PathBuf;

use asciicut_core::{ParseError, ProjectError};

/// A structured bridge failure.
#[derive(Debug)]
pub enum BridgeError {
    /// An operation needing a loaded cast was called before one was loaded.
    /// **Desktop-only** failure mode: the axum server's `AppState` always has
    /// a cast loaded at construction, so its adapter never surfaces this
    /// variant in practice.
    NoCastLoaded,
    /// The source `.cast` could not be read from disk (during
    /// [`crate::Session::load`]).
    ReadCast {
        /// The path that failed to read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The source `.cast` failed to parse (during [`crate::Session::load`]).
    ParseCast(ParseError),
    /// The request-body edit project failed to parse/validate.
    InvalidProject(ProjectError),
    /// A persisted `.asciicut.json` on disk failed to parse/validate, or was
    /// not valid JSON at all — surfaced as one variant since both cases are a
    /// corrupt-persisted-file condition to every caller (the server maps both
    /// to `422`, never a silent "absent").
    InvalidPersistedProject(String),
    /// A read or write to a session/server-derived path failed for a reason
    /// other than "file does not exist" (that case is handled by the caller,
    /// not an error — e.g. `project_meta`'s "no project saved yet").
    Io {
        /// The path the I/O operation targeted.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NoCastLoaded => write!(f, "no cast loaded"),
            BridgeError::ReadCast { path, source } => {
                write!(f, "cannot read cast '{}': {source}", path.display())
            }
            BridgeError::ParseCast(e) => write!(f, "invalid source cast: {e}"),
            BridgeError::InvalidProject(e) => write!(f, "invalid project: {e}"),
            BridgeError::InvalidPersistedProject(msg) => {
                write!(f, "persisted project is invalid: {msg}")
            }
            BridgeError::Io { path, source } => {
                write!(f, "cannot access '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BridgeError::ReadCast { source, .. } | BridgeError::Io { source, .. } => Some(source),
            BridgeError::ParseCast(e) => Some(e),
            BridgeError::InvalidProject(e) => Some(e),
            BridgeError::NoCastLoaded | BridgeError::InvalidPersistedProject(_) => None,
        }
    }
}
