//! Frame-at-T — replay a parsed [`Cast`] up to a source time `T` and snapshot the
//! terminal screen.
//!
//! This is `asciicut-core`'s second load-bearing engine primitive (after the
//! [`cast`](crate::cast) parser). [`frame_at`] walks the normalized event stream
//! in order, applies every event whose absolute [`Event::time`] is `<= T` through
//! asciinema's [`avt`] virtual terminal, and returns an owned [`Frame`] exposing
//! the visible grid both as text (one padded string per visible row) and as
//! styled [`Cell`]s. The filmstrip, segment-boundary previews, scrubbing, and the
//! agent `frame` tool (SPEC §8.1) all build on this primitive.
//!
//! ## Purity / parity discipline (SPEC §7.1)
//!
//! Like the parser, this module is **pure**: model in, frame out. It performs no
//! `std::fs`, time, thread, or process I/O, so it compiles byte-identically to
//! `wasm32-unknown-unknown`. The single shared `avt` VT is precisely what keeps
//! every surface's grid byte-identical rather than threatening parity.
//!
//! ## Two contracts worth pinning
//!
//! 1. **Grid comes from [`Vt::view`], not [`Vt::text`].** `Vt::text()` reads the
//!    *primary* buffer over *scrollback + visible* rows with wrapped rows joined,
//!    so neither its length nor its content match the on-screen grid. `view()`
//!    yields exactly the current visible buffer (alt-screen included). Deriving
//!    **both** the row strings and the cell grid from one `view()` snapshot
//!    guarantees they always describe the same screen, and makes
//!    `frame.text().len() == frame.height()` true by construction.
//! 2. **Pad-to-width row strings.** Each row string is padded with spaces to
//!    exactly `width` display columns. This is a deliberate byte-level parity
//!    contract (pinned by tests), not an accident of avt's internal trimming.
//!
//! ## Untrusted input
//!
//! A `.cast` is untrusted: avt panics on zero dimensions, so zero header dims are
//! clamped to `1` and a `"0x0"`/malformed `r` resize payload is a recoverable
//! no-op. The pure engine never panics on a hostile or degenerate recording.

use avt::Vt;

use crate::cast::{Cast, EventCode};

/// An owned snapshot of the terminal screen at a source time `T`.
///
/// Produced by [`frame_at`]. Holds the visible grid as both padded row strings
/// ([`Frame::text`]) and styled cells ([`Frame::cells`]) — both derived from the
/// same [`Vt::view`] pass, so they always describe the same screen — plus the
/// cursor, the effective dimensions, and the active marker label.
///
/// `Frame` carries **no `f64` fields** (float time stays confined to the replay
/// walk), so it derives a sound [`PartialEq`]: the same `(cast, T)` yields an
/// equal `Frame` on every surface and on repeated calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    width: u16,
    height: u16,
    rows: Vec<String>,
    cells: Vec<Vec<Cell>>,
    cursor: Cursor,
    marker: Option<String>,
}

impl Frame {
    /// The visible grid as one space-padded string per visible row.
    ///
    /// `rows.len() == self.height()` by construction (derived from
    /// [`Vt::view`]), and each row is exactly `self.width()` display columns wide.
    #[must_use]
    pub fn text(&self) -> &[String] {
        &self.rows
    }

    /// The visible grid as styled cells, row-major, derived from the same
    /// [`Vt::view`] snapshot as [`Frame::text`].
    ///
    /// Wide-character trailing cells are elided (the wide head carries
    /// [`Cell::width`] `== 2`), so each row's cell list agrees column-for-column
    /// with its [`Frame::text`] string.
    #[must_use]
    pub fn cells(&self) -> &[Vec<Cell>] {
        &self.cells
    }

    /// The effective terminal width (columns) of this frame after any replayed
    /// resize and the zero-dim clamp.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The effective terminal height (rows) of this frame after any replayed
    /// resize and the zero-dim clamp. Equal to `self.text().len()`.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// The cursor position and visibility at `T`.
    #[must_use]
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// The most recent marker (`m`) label at or before `T`, if any.
    ///
    /// Markers are honored on replay but never fed to the terminal (feeding a
    /// label as output would corrupt the grid).
    #[must_use]
    pub fn marker(&self) -> Option<&str> {
        self.marker.as_deref()
    }
}

/// A single styled grid cell: the printed character plus its display width and
/// visual style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The printed character (a blank cell is a space).
    pub ch: char,
    /// Display columns this cell occupies: `1` for a normal cell, `2` for the
    /// head of a wide (double-width) character.
    pub width: u8,
    /// The visual style (colors + attributes) applied to this cell.
    pub style: Style,
}

/// The visual style of a [`Cell`] — foreground/background color and the boolean
/// text attributes, flattened out of avt's `Pen` into an owned, comparable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    /// Foreground color, if set (else the terminal default).
    pub foreground: Option<Color>,
    /// Background color, if set (else the terminal default).
    pub background: Option<Color>,
    /// Bold intensity.
    pub bold: bool,
    /// Faint (dim) intensity.
    pub faint: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strikethrough.
    pub strikethrough: bool,
    /// Blink.
    pub blink: bool,
    /// Inverse (reverse video).
    pub inverse: bool,
}

/// A terminal color: either a 256-color palette index or a 24-bit RGB triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// A palette index (`0..=255`).
    Indexed(u8),
    /// A 24-bit true-color value.
    Rgb(u8, u8, u8),
}

/// The cursor position (0-based column/row) and visibility at `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// 0-based column.
    pub col: u16,
    /// 0-based row.
    pub row: u16,
    /// Whether the cursor is currently visible.
    pub visible: bool,
}

/// Render the terminal screen at source time `t` by replaying `cast`'s events
/// `0..=t` through the `avt` virtual terminal.
///
/// Events are applied in order while [`Event::time`](crate::Event::time) `<= t`;
/// the first event past `t` stops the walk. `o` output is fed to the terminal,
/// `r` resize changes the effective dimensions, `m` markers are recorded (never
/// fed), and input `i` / unknown codes are skipped. A `t` before the first event
/// yields a blank grid at the (clamped) header dimensions; a `t` at or after the
/// last event replays everything.
///
/// The `.cast` is treated as untrusted: zero header dimensions are clamped to `1`
/// and a `"0x0"`/malformed resize payload is a no-op, so this never panics.
///
/// This is pure and deterministic — the same `(cast, t)` always yields an equal
/// [`Frame`].
///
/// # Examples
///
/// ```
/// let src = "{\"version\": 2, \"width\": 20, \"height\": 3}\n[0.5, \"o\", \"hello\"]\n";
/// let cast = asciicut_core::Cast::parse(src).unwrap();
/// let frame = asciicut_core::frame_at(&cast, 1.0);
/// assert_eq!(frame.height(), 3);
/// assert!(frame.text()[0].starts_with("hello"));
/// ```
#[must_use]
pub fn frame_at(cast: &Cast, t: f64) -> Frame {
    // Seed the VT with clamped header dims. `width`/`height` are `u16`; a `0`
    // (degenerate/untrusted) would make avt panic, so clamp to at least 1.
    let seed_cols = usize::from(cast.header.width.max(1));
    let seed_rows = usize::from(cast.header.height.max(1));
    let mut vt = Vt::builder().size(seed_cols, seed_rows).build();

    let mut marker: Option<String> = None;

    for event in &cast.events {
        // Absolute monotonic `f64` after T02 normalization: a raw prefix compare.
        // `<=` means an event landing exactly on `t` is included.
        if event.time > t {
            break;
        }
        match &event.code {
            EventCode::Output => {
                // Data is already unescaped by the parser.
                let _ = vt.feed_str(&event.data);
            }
            EventCode::Resize => {
                if let Some((cols, rows)) = parse_resize(&event.data) {
                    let _ = vt.resize(cols, rows);
                }
                // A malformed payload or a zero dimension is a recoverable no-op:
                // never call avt with a zero dim (it panics), never abort the frame.
            }
            EventCode::Marker => {
                // Honored on replay but not fed to the terminal — the frame exposes
                // the most recent label at or before `t`.
                marker = Some(event.data.clone());
            }
            EventCode::Other(_) => {
                // Input `i` and unknown codes are not screen output — skip.
            }
        }
    }

    snapshot(&vt, marker)
}

/// Snapshot the VT's *visible* buffer into an owned [`Frame`].
///
/// Both the row strings and the cell grid come from the single `vt.view()` pass,
/// so they always describe the same screen. Effective dims come from `vt.size()`
/// (the source of truth after any resize/clamp).
fn snapshot(vt: &Vt, marker: Option<String>) -> Frame {
    let (eff_cols, eff_rows) = vt.size();

    let mut rows: Vec<String> = Vec::with_capacity(eff_rows);
    let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(eff_rows);

    for line in vt.view() {
        let mut text = String::new();
        let mut row_cells: Vec<Cell> = Vec::new();
        let mut used_cols: usize = 0;

        for cell in line.cells() {
            let width = cell.width();
            // Wide-tail cells (width 0) are occupancy markers — elide them so the
            // text and cell views agree column-for-column.
            if width == 0 {
                continue;
            }
            text.push(cell.char());
            used_cols += usize::from(width);
            row_cells.push(convert_cell(cell));
        }

        // Pad-to-width: each row string is exactly `eff_cols` display columns wide
        // (deliberate byte-level parity contract, pinned by tests).
        if used_cols < eff_cols {
            text.extend(std::iter::repeat_n(' ', eff_cols - used_cols));
        }

        rows.push(text);
        cells.push(row_cells);
    }

    let c = vt.cursor();
    let cursor = Cursor {
        col: clamp_u16(c.col),
        row: clamp_u16(c.row),
        visible: c.visible,
    };

    Frame {
        width: clamp_u16(eff_cols),
        height: clamp_u16(eff_rows),
        rows,
        cells,
        cursor,
        marker,
    }
}

/// Convert one borrowed `avt::Cell` into an owned [`Cell`].
fn convert_cell(cell: &avt::Cell) -> Cell {
    let pen = cell.pen();
    let style = Style {
        foreground: pen.foreground().map(convert_color),
        background: pen.background().map(convert_color),
        bold: pen.is_bold(),
        faint: pen.is_faint(),
        italic: pen.is_italic(),
        underline: pen.is_underline(),
        strikethrough: pen.is_strikethrough(),
        blink: pen.is_blink(),
        inverse: pen.is_inverse(),
    };
    Cell {
        ch: cell.char(),
        width: cell.width(),
        style,
    }
}

/// Map an `avt::Color` onto our owned [`Color`].
fn convert_color(color: avt::Color) -> Color {
    match color {
        avt::Color::Indexed(i) => Color::Indexed(i),
        avt::Color::RGB(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

/// Widen a small terminal coordinate/dimension (`usize`) to `u16`, saturating.
///
/// Grid dims and cursor coords are always small; saturation is a defensive belt
/// so an absurd value can never wrap.
fn clamp_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Parse an `r` resize payload `"<cols>x<rows>"` into non-zero `(cols, rows)`.
///
/// Returns `None` (a no-op) for a malformed payload or a zero dimension. Dims are
/// parsed as `u16` (matching the header), so an absurd payload fails to parse
/// into the no-op path instead of requesting a multi-gigabyte grid.
fn parse_resize(data: &str) -> Option<(usize, usize)> {
    let (cols_str, rows_str) = data.split_once('x')?;
    let cols: u16 = cols_str.trim().parse().ok()?;
    let rows: u16 = rows_str.trim().parse().ok()?;
    if cols == 0 || rows == 0 {
        return None;
    }
    Some((usize::from(cols), usize::from(rows)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Cast {
        Cast::parse(src).expect("fixture should parse")
    }

    #[test]
    fn blank_grid_before_first_event() {
        let cast =
            parse("{\"version\": 2, \"width\": 10, \"height\": 3}\n[2.0, \"o\", \"hello\"]\n");
        // T before the first event: blank grid at header dims.
        let frame = frame_at(&cast, 0.0);
        assert_eq!(frame.width(), 10);
        assert_eq!(frame.height(), 3);
        assert_eq!(frame.text().len(), 3);
        for row in frame.text() {
            assert_eq!(row, "          "); // 10 spaces (pad-to-width)
        }
    }

    #[test]
    fn negative_t_is_empty_grid() {
        let cast = parse("{\"version\": 2, \"width\": 5, \"height\": 2}\n[0.0, \"o\", \"hi\"]\n");
        let frame = frame_at(&cast, -1.0);
        assert!(frame.text().iter().all(|r| r.trim().is_empty()));
    }

    #[test]
    fn output_replayed_to_known_text() {
        let cast =
            parse("{\"version\": 2, \"width\": 10, \"height\": 2}\n[0.5, \"o\", \"hello\"]\n");
        let frame = frame_at(&cast, 1.0);
        assert_eq!(frame.text().len(), usize::from(frame.height()));
        assert_eq!(frame.text()[0], "hello     "); // padded to width 10
        assert_eq!(frame.text()[1], "          ");
    }

    #[test]
    fn boundary_t_includes_event_exactly_on_t() {
        // Marker and output both land exactly at T=0.5 — `<=` includes both.
        let cast = parse(
            "{\"version\": 2, \"width\": 10, \"height\": 2}\n\
             [0.5, \"o\", \"hi\"]\n\
             [0.5, \"m\", \"chapter\"]\n\
             [0.6, \"o\", \"XX\"]\n",
        );
        let frame = frame_at(&cast, 0.5);
        assert_eq!(frame.text()[0], "hi        ");
        assert_eq!(frame.marker(), Some("chapter"));
        // The 0.6 output is *not* included at T=0.5.
        assert!(!frame.text()[0].contains("XX"));
    }

    #[test]
    fn huge_t_replays_last_frame() {
        let cast = parse("{\"version\": 2, \"width\": 6, \"height\": 2}\n[1.0, \"o\", \"done\"]\n");
        let frame = frame_at(&cast, 1e9);
        assert_eq!(frame.text()[0], "done  ");
    }

    #[test]
    fn scrolling_shows_visible_tail_not_history() {
        // 3 rows; feed 5 newline-terminated lines so it scrolls past `height`.
        // This is the guard that would have caught a `text()`-vs-`view()` defect.
        let cast = parse(
            "{\"version\": 2, \"width\": 10, \"height\": 3}\n\
             [0.1, \"o\", \"L1\\r\\n\"]\n\
             [0.2, \"o\", \"L2\\r\\n\"]\n\
             [0.3, \"o\", \"L3\\r\\n\"]\n\
             [0.4, \"o\", \"L4\\r\\n\"]\n\
             [0.5, \"o\", \"L5\\r\\n\"]\n",
        );
        let frame = frame_at(&cast, 1.0);
        // Exactly `height` visible rows, never the full scroll history.
        assert_eq!(frame.text().len(), 3);
        assert_eq!(usize::from(frame.height()), frame.text().len());
        // Visible tail: L4, L5, then a blank row.
        assert_eq!(frame.text()[0], "L4        ");
        assert_eq!(frame.text()[1], "L5        ");
        assert_eq!(frame.text()[2], "          ");
        // The scrolled-off history is gone from the visible grid.
        let joined = frame.text().join("\n");
        assert!(!joined.contains("L1"));
        assert!(!joined.contains("L2"));
    }

    #[test]
    fn wrapped_row_pads_to_width_and_cells_agree() {
        // Feed more columns than `width` (5) so the row wraps; both rows must be
        // exactly `width` wide and the cell view must agree with the text view.
        let cast =
            parse("{\"version\": 2, \"width\": 5, \"height\": 3}\n[0.1, \"o\", \"abcdefg\"]\n");
        let frame = frame_at(&cast, 1.0);
        assert_eq!(frame.text()[0], "abcde");
        assert_eq!(frame.text()[1], "fg   ");
        for (row_str, row_cells) in frame.text().iter().zip(frame.cells()) {
            assert_eq!(row_str.chars().count(), 5, "each row is exactly width wide");
            let from_cells: String = row_cells.iter().map(|c| c.ch).collect();
            assert_eq!(
                &from_cells, row_str,
                "cells and text agree column-for-column"
            );
        }
    }

    #[test]
    fn resize_changes_effective_dims() {
        let cast = parse(
            "{\"version\": 2, \"width\": 10, \"height\": 4}\n\
             [0.1, \"o\", \"x\"]\n\
             [0.2, \"r\", \"20x6\"]\n",
        );
        // Before the resize.
        let before = frame_at(&cast, 0.15);
        assert_eq!((before.width(), before.height()), (10, 4));
        // After the resize.
        let after = frame_at(&cast, 0.5);
        assert_eq!((after.width(), after.height()), (20, 6));
        assert_eq!(after.text().len(), 6);
        assert!(after.text().iter().all(|r| r.chars().count() == 20));
    }

    #[test]
    fn zero_header_dims_clamp_without_panic() {
        let cast = parse("{\"version\": 2, \"width\": 0, \"height\": 0}\n[0.1, \"o\", \"hi\"]\n");
        let frame = frame_at(&cast, 1.0);
        // Clamped to at least 1x1 — no panic.
        assert!(frame.width() >= 1);
        assert!(frame.height() >= 1);
        assert_eq!(frame.text().len(), usize::from(frame.height()));
    }

    #[test]
    fn malformed_and_zero_resize_are_noops() {
        let cast = parse(
            "{\"version\": 2, \"width\": 8, \"height\": 3}\n\
             [0.1, \"r\", \"0x0\"]\n\
             [0.2, \"r\", \"not-a-size\"]\n\
             [0.3, \"r\", \"99999999x1\"]\n\
             [0.4, \"o\", \"ok\"]\n",
        );
        // None of the bogus resizes panic or change the dims.
        let frame = frame_at(&cast, 1.0);
        assert_eq!((frame.width(), frame.height()), (8, 3));
        assert_eq!(frame.text()[0], "ok      ");
    }

    #[test]
    fn marker_honored_without_corrupting_grid() {
        let cast = parse(
            "{\"version\": 2, \"width\": 10, \"height\": 2}\n\
             [0.1, \"o\", \"line\"]\n\
             [0.2, \"m\", \"the-label\"]\n",
        );
        let frame = frame_at(&cast, 1.0);
        // The label is exposed but never fed to the grid.
        assert_eq!(frame.marker(), Some("the-label"));
        assert_eq!(frame.text()[0], "line      ");
        assert!(!frame.text().iter().any(|r| r.contains("the-label")));
    }

    #[test]
    fn latest_marker_at_or_before_t_wins() {
        let cast = parse(
            "{\"version\": 2, \"width\": 5, \"height\": 1}\n\
             [0.1, \"m\", \"first\"]\n\
             [0.5, \"m\", \"second\"]\n\
             [0.9, \"m\", \"third\"]\n",
        );
        assert_eq!(frame_at(&cast, 0.0).marker(), None);
        assert_eq!(frame_at(&cast, 0.1).marker(), Some("first"));
        assert_eq!(frame_at(&cast, 0.6).marker(), Some("second"));
        assert_eq!(frame_at(&cast, 5.0).marker(), Some("third"));
    }

    #[test]
    fn input_and_unknown_codes_are_skipped() {
        let cast = parse(
            "{\"version\": 2, \"width\": 10, \"height\": 2}\n\
             [0.1, \"o\", \"out\"]\n\
             [0.2, \"i\", \"typed-input\"]\n\
             [0.3, \"x\", \"garbage\"]\n",
        );
        let frame = frame_at(&cast, 1.0);
        assert_eq!(frame.text()[0], "out       ");
        // Input / unknown codes never touch the grid.
        assert!(!frame.text().iter().any(|r| r.contains("typed-input")));
        assert!(!frame.text().iter().any(|r| r.contains("garbage")));
    }

    #[test]
    fn determinism_same_input_same_frame() {
        let cast = parse(
            "{\"version\": 2, \"width\": 12, \"height\": 3}\n\
             [0.1, \"o\", \"deterministic\"]\n\
             [0.2, \"r\", \"14x4\"]\n\
             [0.3, \"m\", \"mark\"]\n",
        );
        // Frame derives PartialEq (no f64 fields) — repeated calls are equal.
        assert_eq!(frame_at(&cast, 1.0), frame_at(&cast, 1.0));
    }

    #[test]
    fn cell_style_captures_sgr_color() {
        // SGR: bold + red foreground, then "A".
        let cast = parse(
            "{\"version\": 2, \"width\": 4, \"height\": 1}\n[0.1, \"o\", \"\\u001b[1;31mA\"]\n",
        );
        let frame = frame_at(&cast, 1.0);
        let cell = frame.cells()[0][0];
        assert_eq!(cell.ch, 'A');
        assert!(cell.style.bold, "bold attr captured");
        assert_eq!(
            cell.style.foreground,
            Some(Color::Indexed(1)),
            "red fg captured"
        );
    }

    #[test]
    fn cursor_position_tracked() {
        let cast = parse("{\"version\": 2, \"width\": 10, \"height\": 2}\n[0.1, \"o\", \"abc\"]\n");
        let frame = frame_at(&cast, 1.0);
        // After "abc" the cursor sits at column 3, row 0, visible.
        assert_eq!(frame.cursor().col, 3);
        assert_eq!(frame.cursor().row, 0);
        assert!(frame.cursor().visible);
    }
}
