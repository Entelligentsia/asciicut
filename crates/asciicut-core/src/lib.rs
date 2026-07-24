//! `asciicut-core` — the single shared asciicut engine crate.
//!
//! This crate holds the virtual-terminal (frame-at-T) and compose logic that
//! every asciicut surface — CLI, MCP tools, the axum server, and the zero-install
//! WASM web demo — builds on (SPEC §7.2, §7.4). It compiles identically to the
//! native host target and to `wasm32-unknown-unknown`, and is deliberately kept
//! free of wasm-incompatible surface (no `std::fs`, threads, time, or process
//! APIs) so the byte-identical native ↔ wasm parity contract (SPEC §7.1) holds
//! from day one.
//!
//! The public surface is the asciinema [`cast`] parser — which turns a v2/v3
//! `.cast` into one version-normalized [`Cast`] model (header +
//! absolute-timestamped events) and can serialize it back to a v2 document
//! ([`Cast::to_cast_string`]); the [`frame`] engine, which replays that model
//! through asciinema's `avt` virtual terminal to produce the screen [`Frame`] at
//! any source time `T`; the [`activity`] primitive, which reduces the same model
//! to a per-bucket change-density waveform ([`ActivitySignal`]) for the
//! timeline/scrubbing UI; and the [`compose`] engine, which projects an edit
//! [`Project`] onto a source cast (keep-segments, per-segment speed, global idle
//! cap, end-holds, inter-segment beats) into a fresh composed [`Cast`].

/// Asciinema `.cast` v2/v3 parser: pure string-in / model-out, wasm-parity safe.
pub mod cast;

/// Frame-at-T: replay a [`Cast`] through the `avt` VT and snapshot the screen.
pub mod frame;

/// Activity signal: reduce a [`Cast`] to a per-bucket change-density waveform.
pub mod activity;

/// Compose engine: project an edit [`Project`] onto a source [`Cast`].
pub mod compose;

pub use activity::{activity_signal, ActivitySignal, DEFAULT_BUCKET_SECS};
pub use cast::{Cast, Event, EventCode, Header, ParseError};
pub use compose::{compose, Marker, Output, Project, ProjectError, Segment};
pub use frame::{frame_at, Cell, Color, Cursor, Frame, Style};

/// The crate's semantic version, as declared in `Cargo.toml`.
///
/// A tiny platform-agnostic accessor that gives the build/test/clippy gates a
/// real public item to check without pre-empting the T03/T05 engine work.
///
/// # Examples
///
/// ```
/// assert!(!asciicut_core::version().is_empty());
/// ```
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_reported() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
        assert!(!version().is_empty());
    }
}
