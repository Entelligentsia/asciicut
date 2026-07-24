//! Integration tests for the frame-at-T engine over on-disk fixtures.
//!
//! These read real `.cast` files from disk (which the pure `asciicut_core::frame`
//! module never does) via `CARGO_MANIFEST_DIR`: the real v2 recording shipped at
//! the repo root (`samples/sample.cast`) plus small hand-written fixtures whose
//! grids are exactly reviewer-checkable — including the scrolling guard that
//! pins the `view()`-not-`text()` contract.

use std::fs;
use std::path::PathBuf;

use asciicut_core::{frame_at, Cast, Frame};

/// The display-column width of every row, computed from cell widths.
///
/// NB: `String::chars().count()` is *not* the display width — a double-width glyph
/// is one `char` but two columns — so parity is checked against the summed cell
/// widths, which is exactly what the pad-to-width policy targets.
fn row_display_widths(frame: &Frame) -> Vec<usize> {
    frame
        .cells()
        .iter()
        .map(|row| row.iter().map(|c| usize::from(c.width)).sum())
        .collect()
}

/// Read a fixture next to this test file (`tests/fixtures/<name>`).
fn read_fixture(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Read the real v2 `samples/sample.cast` from the repo root.
fn read_sample_v2() -> String {
    // CARGO_MANIFEST_DIR is `<repo>/crates/asciicut-core`; the sample lives two
    // levels up at the workspace root.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // repo root
    path.push("samples/sample.cast");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn scroll_fixture_shows_visible_tail() {
    // The Blocker-2 guard on disk: a 3-row terminal fed 5 lines. If the grid came
    // from `vt.text()` (scrollback-bleeding) instead of `vt.view()`, this would
    // return all 5 history lines and the length/tail assertions would fail.
    let cast = Cast::parse(&read_fixture("scroll.cast")).unwrap();
    let frame = frame_at(&cast, 1.0);

    assert_eq!(frame.text().len(), 3, "exactly `height` visible rows");
    assert_eq!(usize::from(frame.height()), frame.text().len());
    assert_eq!(frame.text()[0], "L4        ");
    assert_eq!(frame.text()[1], "L5        ");
    assert_eq!(frame.text()[2], "          ");

    let joined = frame.text().join("\n");
    assert!(!joined.contains("L1"), "scrolled-off history must be gone");
    assert!(!joined.contains("L2"), "scrolled-off history must be gone");
}

#[test]
fn resize_marker_fixture_honors_r_and_m() {
    let cast = Cast::parse(&read_fixture("resize_marker.cast")).unwrap();

    // Before the resize (T between the marker `intro` and the resize): 8x3.
    let early = frame_at(&cast, 0.25);
    assert_eq!((early.width(), early.height()), (8, 3));
    assert_eq!(early.marker(), Some("intro"));
    assert_eq!(early.text()[0].trim_end(), "hello");
    // The marker label never bleeds into the grid.
    assert!(!early.text().iter().any(|r| r.contains("intro")));

    // After the full replay: resized to 12x4, latest marker `outro`, and both
    // output lines present at the new width.
    let late = frame_at(&cast, 1.0);
    assert_eq!((late.width(), late.height()), (12, 4));
    assert_eq!(late.text().len(), 4);
    assert!(late.text().iter().all(|r| r.chars().count() == 12));
    assert_eq!(late.marker(), Some("outro"));
    assert_eq!(late.text()[0].trim_end(), "hello");
    assert_eq!(late.text()[1].trim_end(), "world");
}

#[test]
fn sample_cast_final_frame_is_stable() {
    // A real-cast smoke: replay the whole recording. The sample resizes
    // 120x40 -> 200x40 -> 120x40, so the final frame is 120x40.
    let cast = Cast::parse(&read_sample_v2()).unwrap();
    let frame = frame_at(&cast, 1e9);

    assert_eq!((frame.width(), frame.height()), (120, 40));
    assert_eq!(frame.text().len(), usize::from(frame.height()));
    // Every row fills the effective width in *display columns* (wide glyphs count
    // as two columns / one char, so this is checked via cell widths, not chars).
    assert!(row_display_widths(&frame).iter().all(|&w| w == 120));
    // The grid is not entirely blank at the end of a real recording.
    assert!(
        frame.text().iter().any(|r| r.trim().chars().count() > 0),
        "final frame should have visible content"
    );
}

#[test]
fn sample_cast_midpoint_frame_is_well_formed() {
    // Mid-recording: dims are one of the recorded sizes, grid is well-formed,
    // and text().len() always equals the effective height.
    let cast = Cast::parse(&read_sample_v2()).unwrap();
    let frame = frame_at(&cast, 5.0);

    assert_eq!(frame.height(), 40);
    assert!(frame.width() == 120 || frame.width() == 200);
    assert_eq!(frame.text().len(), usize::from(frame.height()));
    assert!(row_display_widths(&frame)
        .iter()
        .all(|&w| w == usize::from(frame.width())));
}

#[test]
fn sample_cast_replay_is_deterministic() {
    let cast = Cast::parse(&read_sample_v2()).unwrap();
    // Same (cast, T) -> identical Frame (Frame derives PartialEq; no f64 fields).
    assert_eq!(frame_at(&cast, 5.0), frame_at(&cast, 5.0));
}
