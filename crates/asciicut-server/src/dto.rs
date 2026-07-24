//! HTTP wire DTOs — re-exported from [`asciicut_bridge::dto`] (CASTCU-S2-T02).
//!
//! These DTOs used to be defined here, mapped directly from `asciicut-core`'s
//! frame types. They now live in `asciicut-bridge` so both this axum surface
//! and the Tauri desktop command surface serialize the exact same `Serialize`
//! impl (D2 — no two hand-kept-in-sync DTO sets). Re-exported under this
//! module path so nothing importing `asciicut_server::dto` breaks.

pub use asciicut_bridge::dto::{
    ActivitySignalDto, CellDto, ColorDto, CursorDto, FrameDto, StyleDto, ThumbDto,
};
