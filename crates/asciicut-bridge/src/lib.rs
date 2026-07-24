//! `asciicut-bridge` — the transport-agnostic command bridge (CASTCU-S2-T02).
//!
//! Both `asciicut-server` (axum HTTP) and `asciicut-desktop` (Tauri commands)
//! call the *same* functions in [`ops`] over the *same* [`Session`] type —
//! there is exactly one implementation of frame/thumbs/activity/compose/
//! project/save, not two hand-kept-in-sync ones (plan AC#1). This crate is
//! deliberately free of `axum`/`tokio`/HTTP types: no dependency on either
//! lives in `Cargo.toml`, and [`BridgeError`] is the only failure surface —
//! each transport maps it to its own error convention (an HTTP status code, or
//! a Tauri command's `Result` rejection).
//!
//! ## Layering
//!
//! - [`dto`] — the wire DTOs (`FrameDto`/`ThumbDto`/`ActivitySignalDto`/
//!   `CellDto`/`StyleDto`/`ColorDto`/`CursorDto`/`SaveResponse`/`ProjectMeta`),
//!   relocated verbatim from `asciicut-server::dto` (same field names, same
//!   `serde` attributes) so both transports emit provably identical JSON.
//! - [`session`] — [`Session`], the in-process parsed-cast-plus-path state.
//!   Unlike the axum server's `AppState` (which always has a cast, resolved at
//!   launch), a bridge `Session` may start **empty**: the desktop shell may
//!   have no cast loaded until a launch argument or a future native Open
//!   supplies one.
//! - [`ops`] — one plain-Rust operation per data call (`frame`, `thumbs`,
//!   `activity`, `event_times`, `compose_project`, `save_project`,
//!   `project_meta`), each a
//!   function over a [`Session`] and `asciicut-core`, returning a bridge DTO or
//!   a [`BridgeError`]. No `axum` types, no HTTP status codes.
//! - [`error`] — [`BridgeError`], the structured failure enum.
//!
//! `/api/export` is explicitly **not** part of this bridge — it stays an
//! axum-only handler in `asciicut-server` (out of this task's scope; the
//! desktop export story is a different `agg`/`ffmpeg`-backed pipeline landing
//! in a later task, not a DTO-bridge of the existing JSON-project-writer).

pub mod dto;
mod error;
pub mod ops;
mod session;

pub use error::BridgeError;
pub use session::{sibling_path, Session, COMPOSED_CAST_SUFFIX, PROJECT_SUFFIX};
