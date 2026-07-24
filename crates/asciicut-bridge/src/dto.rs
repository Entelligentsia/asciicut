//! Wire DTOs — the transport-agnostic serialization contract, mapped *from*
//! `asciicut-core`'s public frame types.
//!
//! `asciicut-core`'s [`Frame`]/[`Cell`]/[`Style`]/[`Color`]/[`Cursor`] carry no
//! `serde` derives (coupling the WASM serialization shape to a presentation
//! wire format now would be premature and would disturb the native ↔ wasm
//! parity contract). So the bridge owns these thin, `serde`-serializable
//! presentation types and a total `From`/mapping into them. This keeps the
//! domain model and the wire format independently evolvable.
//!
//! Relocated verbatim (same field names, same `serde` attributes) from
//! `asciicut-server::dto` — both the axum `/api/*` surface and the Tauri
//! command surface serialize the *same* `Serialize` impl, so the JSON either
//! transport emits is provably identical (CASTCU-S2-T02 D2/AC#1).

use asciicut_core::{ActivitySignal, Cell, Color, Cursor, Frame, Style};
use serde::Serialize;

/// A styled terminal screen snapshot at one source time `T`, ready for the SPA
/// canvas. Row-major cells plus the padded row strings (both derived from the
/// same core [`Frame`]), the effective dimensions, cursor, and active marker.
#[derive(Debug, Clone, Serialize)]
pub struct FrameDto {
    /// Effective terminal width in columns.
    pub width: u16,
    /// Effective terminal height in rows.
    pub height: u16,
    /// One space-padded string per visible row (`text.len() == height`).
    pub text: Vec<String>,
    /// The visible grid as styled cells, row-major.
    pub cells: Vec<Vec<CellDto>>,
    /// The cursor position and visibility.
    pub cursor: CursorDto,
    /// The most recent marker label at or before `T`, if any.
    pub marker: Option<String>,
}

/// The change-density waveform for the launch cast: the bucket duration plus the
/// ordered per-bucket score array. Mapped *from* core's [`ActivitySignal`] via
/// its public accessors (the core fields are private and carry no `serde`
/// derives). The client derives `duration` from `buckets.len() * bucket_secs`.
#[derive(Debug, Clone, Serialize)]
pub struct ActivitySignalDto {
    /// The bucket duration in seconds every index is measured against.
    pub bucket_secs: f64,
    /// The ordered per-bucket change-density scores (the waveform array).
    pub buckets: Vec<u64>,
}

/// One filmstrip sample: the source time and the frame at that time.
#[derive(Debug, Clone, Serialize)]
pub struct ThumbDto {
    /// The sampled source time in seconds.
    pub t: f64,
    /// The frame snapshot at `t`.
    pub frame: FrameDto,
}

/// A single styled grid cell.
#[derive(Debug, Clone, Serialize)]
pub struct CellDto {
    /// The printed character (a blank cell is a space).
    pub ch: char,
    /// Display columns: `1` for a normal cell, `2` for a wide-character head.
    pub width: u8,
    /// The cell's visual style.
    pub style: StyleDto,
}

/// Foreground/background color plus the boolean text attributes of a [`CellDto`].
/// Attribute flags are skipped when `false` to keep the wire payload compact.
#[derive(Debug, Clone, Serialize)]
pub struct StyleDto {
    /// Foreground color, if set (else terminal default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<ColorDto>,
    /// Background color, if set (else terminal default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<ColorDto>,
    /// Bold intensity.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Faint (dim) intensity.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub faint: bool,
    /// Italic.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Underline.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Strikethrough.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strikethrough: bool,
    /// Blink.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub blink: bool,
    /// Inverse (reverse video).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inverse: bool,
}

/// A terminal color, tagged so the SPA can distinguish palette from true-color.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ColorDto {
    /// A 256-color palette index.
    Indexed {
        /// The palette index (`0..=255`).
        index: u8,
    },
    /// A 24-bit true-color value.
    Rgb {
        /// Red channel.
        r: u8,
        /// Green channel.
        g: u8,
        /// Blue channel.
        b: u8,
    },
}

/// Cursor position (0-based) and visibility.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CursorDto {
    /// 0-based column.
    pub col: u16,
    /// 0-based row.
    pub row: u16,
    /// Whether the cursor is currently visible.
    pub visible: bool,
}

/// Response for [`crate::ops::save_project`]: the server/session-derived path
/// written and its byte length. Relocated verbatim from
/// `asciicut-server::routes::SaveResponse` (D2) — fields are `pub` (unlike the
/// original's module-private visibility) since both transports, and their
/// respective test suites, now live in different crates/modules than the
/// constructor in [`crate::ops`].
#[derive(Debug, Serialize)]
pub struct SaveResponse {
    /// The absolute-or-relative path the project was written to.
    pub path: String,
    /// Number of bytes written.
    pub bytes: usize,
}

/// Response for [`crate::ops::project_meta`]: the authoritative launch
/// `source` name plus the persisted project echoed verbatim (or `null` when
/// none is on disk yet). Relocated verbatim from
/// `asciicut-server::routes::ProjectMeta` (D2).
#[derive(Debug, Serialize)]
pub struct ProjectMeta {
    /// The launch cast's file name — the authoritative project `source`.
    pub source: String,
    /// The persisted `.asciicut.json` as a raw JSON value, or `null`.
    pub project: Option<serde_json::Value>,
}

impl From<&ActivitySignal> for ActivitySignalDto {
    fn from(signal: &ActivitySignal) -> Self {
        ActivitySignalDto {
            bucket_secs: signal.bucket_secs(),
            buckets: signal.buckets().to_vec(),
        }
    }
}

impl From<&Frame> for FrameDto {
    fn from(frame: &Frame) -> Self {
        FrameDto {
            width: frame.width(),
            height: frame.height(),
            text: frame.text().to_vec(),
            cells: frame
                .cells()
                .iter()
                .map(|row| row.iter().map(CellDto::from).collect())
                .collect(),
            cursor: CursorDto::from(frame.cursor()),
            marker: frame.marker().map(str::to_owned),
        }
    }
}

impl From<&Cell> for CellDto {
    fn from(cell: &Cell) -> Self {
        CellDto {
            ch: cell.ch,
            width: cell.width,
            style: StyleDto::from(cell.style),
        }
    }
}

impl From<Style> for StyleDto {
    fn from(style: Style) -> Self {
        StyleDto {
            foreground: style.foreground.map(ColorDto::from),
            background: style.background.map(ColorDto::from),
            bold: style.bold,
            faint: style.faint,
            italic: style.italic,
            underline: style.underline,
            strikethrough: style.strikethrough,
            blink: style.blink,
            inverse: style.inverse,
        }
    }
}

impl From<Color> for ColorDto {
    fn from(color: Color) -> Self {
        match color {
            Color::Indexed(index) => ColorDto::Indexed { index },
            Color::Rgb(r, g, b) => ColorDto::Rgb { r, g, b },
        }
    }
}

impl From<Cursor> for CursorDto {
    fn from(cursor: Cursor) -> Self {
        CursorDto {
            col: cursor.col,
            row: cursor.row,
            visible: cursor.visible,
        }
    }
}
