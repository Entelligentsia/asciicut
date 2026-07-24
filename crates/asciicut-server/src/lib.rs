//! `asciicut-server` — the native local HTTP surface (SPEC §7.2).
//!
//! An **axum** server, SPA baked in via **rust-embed**, wrapping the shared
//! [`asciicut_core`] engine. It is a **library crate with no `[[bin]]`** on
//! purpose: keeping the HTTP surface a library leaves `asciicut` as the *only*
//! binary target in the virtual workspace, so `cargo run -- <file.cast>`
//! resolves unambiguously (AC#3). The `asciicut` binary calls [`serve`].
//!
//! ## Layering
//!
//! The frame/thumbs/activity/compose/project/save handler logic and their wire
//! DTOs ([`dto`]) now live in the transport-agnostic `asciicut-bridge` crate
//! (CASTCU-S2-T02) — both this axum surface and the Tauri desktop command
//! surface call the *same* bridge functions, so there is exactly one
//! implementation, not two hand-kept-in-sync ones. This crate is a thin adapter:
//! [`routes`] maps `asciicut_bridge::BridgeError` to the existing HTTP status
//! contract (400/422/500) and wraps the bridge's DTOs in `Json`. `/api/export`
//! is the one exception — it stays server-only (out of the bridge's scope).
//! Core is untouched and the native ↔ wasm parity boundary (SPEC §7.1) stays
//! undisturbed — the `axum`/`tokio`/`rust-embed`/`mime_guess` deps are
//! native-only.
//!
//! ## Surface
//!
//! - `GET  /api/frame?t=<s>`     — the styled screen grid at source time `T`.
//! - `GET  /api/thumbs?count=<n>`— `n` evenly-spaced filmstrip frames.
//! - `GET  /api/activity?bucket=<s>` — the change-density waveform (T09).
//! - `GET  /api/events`          — the cast's real event timestamps.
//! - `POST /api/compose`         — compose an edit project → composed `.cast`.
//! - `POST /api/save`            — persist a project to a server-derived path.
//! - `GET  /api/project`         — the launch source name + any persisted project.
//! - `POST /api/export`          — write the project + composed `.cast` (T13).
//! - everything else             — the embedded SPA (`index.html` fallback).
//!
//! ## Testability
//!
//! [`router`] and [`AppState`] are public so integration tests drive the full
//! request path in-process via `tower`'s `oneshot` — no real socket bind.

mod assets;
pub mod dto;
mod routes;

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asciicut_bridge::{BridgeError, Session};
use asciicut_core::{Cast, ParseError};
use axum::{
    routing::{get, post},
    Router,
};

/// Read-mostly server state: a [`asciicut_bridge::Session`] (the parsed source
/// [`Cast`] plus its on-disk path), loaded once at launch and shared across
/// requests via [`Arc`].
///
/// The server's own invariant — unlike the desktop shell's bridge session — is
/// that this `Session` is **always loaded**: both constructors below build it
/// with a cast already present, matching the server's launch contract
/// (`serve` always resolves a cast before building state). `routes.rs` and the
/// accessors below rely on that invariant rather than propagating
/// [`BridgeError::NoCastLoaded`] through every handler.
#[derive(Debug)]
pub struct AppState {
    session: Session,
}

impl AppState {
    /// Build state directly from an already-parsed cast and its path (test entry
    /// / in-process construction).
    #[must_use]
    pub fn new(cast: Cast, cast_path: PathBuf) -> Self {
        AppState {
            session: Session::with_cast(cast, cast_path),
        }
    }

    /// Read and parse the `.cast` at `cast_path` into server state.
    ///
    /// # Errors
    ///
    /// Returns [`ServeError::ReadCast`] if the file is unreadable and
    /// [`ServeError::ParseCast`] if it fails to parse.
    pub fn load(cast_path: PathBuf) -> Result<Self, ServeError> {
        let mut session = Session::new();
        session.load(cast_path).map_err(ServeError::from_bridge)?;
        Ok(AppState { session })
    }

    /// The bridge [`Session`] this state wraps — what `routes.rs` passes to
    /// every `asciicut_bridge::ops` call.
    pub(crate) fn session(&self) -> &Session {
        &self.session
    }

    /// The loaded cast's on-disk path. Server `AppState` always has a cast
    /// loaded (see the struct-level invariant note) — a code invariant, not a
    /// user-reachable failure mode, so the `NoCastLoaded` case is `expect`ed
    /// away here rather than threaded through every caller.
    fn cast_path(&self) -> &Path {
        self.session
            .cast_path()
            .expect("server AppState always has a loaded cast")
    }

    /// The **server-derived** save target: `<cast_dir>/<cast_stem>.asciicut.json`.
    ///
    /// Derived purely from the launch cast path — never from request input — so
    /// `/api/save` cannot be steered at an arbitrary filesystem location
    /// (advisory #1).
    #[must_use]
    pub fn save_path(&self) -> PathBuf {
        self.session
            .save_path()
            .expect("server AppState always has a loaded cast")
    }

    /// The launch cast's **file name** (e.g. `sample.cast`) — the authoritative
    /// project `source` the SPA stamps on load (T13). Falls back to the whole
    /// path string if the OS path has no final component (never expected for a
    /// real launch cast).
    #[must_use]
    pub fn source_name(&self) -> String {
        self.session
            .source_name()
            .expect("server AppState always has a loaded cast")
    }

    /// The **server-derived** composed-cast export target:
    /// `<cast_dir>/<cast_stem>.composed.cast`. Derived purely from the launch
    /// cast path (never request input), and given a distinct `.composed.cast`
    /// name so an export never clobbers the source `.cast` (T13, advisory #1).
    ///
    /// `/api/export` is out of `asciicut-bridge`'s scope (CASTCU-S2-T02 plan),
    /// but the *derivation* is not: it goes through the bridge's
    /// [`asciicut_bridge::sibling_path`], the same one [`Self::save_path`] and
    /// the desktop export pipeline use, so the two surfaces cannot drift apart
    /// on where a composed cast lands.
    #[must_use]
    pub fn export_cast_path(&self) -> PathBuf {
        asciicut_bridge::sibling_path(self.cast_path(), asciicut_bridge::COMPOSED_CAST_SUFFIX)
    }
}

/// Assemble the axum [`Router`] over a shared [`AppState`].
///
/// Kept separate from [`serve`] so tests can drive the whole request path with
/// `tower`'s `oneshot` without binding a socket.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/frame", get(routes::frame))
        .route("/api/thumbs", get(routes::thumbs))
        .route("/api/activity", get(routes::activity))
        .route("/api/events", get(routes::events))
        .route("/api/compose", post(routes::compose_project))
        .route("/api/save", post(routes::save))
        .route("/api/project", get(routes::project))
        .route("/api/export", post(routes::export))
        .fallback(routes::spa_fallback)
        .with_state(state)
}

/// Read `cast_path`, then serve the SPA + `/api` surface on `127.0.0.1:port`.
///
/// The bind address is a **code invariant** of `127.0.0.1` (advisory #2): the
/// server performs file writes on request and must never be reachable off the
/// local host. The bound URL is printed to stdout once the listener is up.
///
/// # Errors
///
/// Returns a [`ServeError`] if the cast cannot be read/parsed, the port cannot
/// be bound, or the server loop exits with an I/O error.
pub async fn serve(cast_path: PathBuf, port: u16) -> Result<(), ServeError> {
    let state = Arc::new(AppState::load(cast_path)?);
    let app = router(state);

    // Loopback-only, always. Never 0.0.0.0 — this process writes files on request.
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| ServeError::Bind { addr, source })?;

    let bound = listener.local_addr().map_err(ServeError::Serve)?;
    println!("asciicut serving on http://{bound}");

    axum::serve(listener, app)
        .await
        .map_err(ServeError::Serve)?;
    Ok(())
}

/// A structured error for the server lifecycle: reading/parsing the cast,
/// binding the port, or the serve loop.
#[derive(Debug)]
pub enum ServeError {
    /// The source `.cast` could not be read from disk.
    ReadCast {
        /// The path that failed to read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The source `.cast` failed to parse.
    ParseCast(ParseError),
    /// The listen address could not be bound.
    Bind {
        /// The address that failed to bind.
        addr: SocketAddr,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The serve loop exited with an I/O error.
    Serve(std::io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServeError::ReadCast { path, source } => {
                write!(f, "cannot read cast '{}': {source}", path.display())
            }
            ServeError::ParseCast(e) => write!(f, "invalid source cast: {e}"),
            ServeError::Bind { addr, source } => write!(f, "cannot bind {addr}: {source}"),
            ServeError::Serve(e) => write!(f, "server error: {e}"),
        }
    }
}

impl std::error::Error for ServeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ServeError::ReadCast { source, .. } | ServeError::Bind { source, .. } => Some(source),
            ServeError::ParseCast(e) => Some(e),
            ServeError::Serve(e) => Some(e),
        }
    }
}

impl ServeError {
    /// Map a [`BridgeError`] from [`Session::load`] into the matching
    /// `ServeError` variant, preserving the exact `cannot read cast '...'`
    /// / `invalid source cast: ...` diagnostics `main.rs` and the CLI's
    /// `bare_cast_argument_routes_to_server` regression test already pin.
    /// `Session::load` only ever returns `ReadCast`/`ParseCast` — the other
    /// `BridgeError` variants (`NoCastLoaded`, project-related) are not
    /// reachable from a cast-load call, so they fall back to `Serve` rather
    /// than being asserted unreachable.
    fn from_bridge(err: BridgeError) -> Self {
        match err {
            BridgeError::ReadCast { path, source } => ServeError::ReadCast { path, source },
            BridgeError::ParseCast(e) => ServeError::ParseCast(e),
            other => ServeError::Serve(std::io::Error::other(other.to_string())),
        }
    }
}
