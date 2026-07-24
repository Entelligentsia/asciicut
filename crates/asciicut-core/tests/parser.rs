//! Integration tests for the `.cast` parser over on-disk fixtures.
//!
//! These read real files from disk (which the pure `asciicut_core::cast` module
//! never does) via `CARGO_MANIFEST_DIR`: the real v2 recording shipped at the
//! repo root (`samples/sample.cast`) and a small hand-written v3 fixture with
//! exact, hand-computable accumulated times.

use std::fs;
use std::path::PathBuf;

use asciicut_core::{Cast, EventCode};

/// Epsilon for float time comparisons — never compare `f64` with `==`.
const EPS: f64 = 1e-6;

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

/// Read the synthetic v3 fixture next to this test file.
fn read_synthetic_v3() -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/synthetic_v3.cast");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn parses_real_v2_sample_header() {
    let cast = Cast::parse(&read_sample_v2()).expect("sample.cast should parse");
    assert_eq!(cast.header.version, 2);
    assert_eq!(cast.header.width, 120);
    assert_eq!(cast.header.height, 40);
    assert_eq!(cast.header.timestamp, Some(1784701971));
    let env = cast.header.env.expect("sample has an env block");
    assert!(env.contains_key("SHELL"));
    assert!(!cast.events.is_empty());
}

#[test]
fn v2_sample_times_are_monotonic_and_first_output_matches() {
    let cast = Cast::parse(&read_sample_v2()).unwrap();

    // The first event is an `o` at ~2.007182 (absolute v2 time, used as-is).
    let first = &cast.events[0];
    assert_eq!(first.code, EventCode::Output);
    assert!(
        (first.time - 2.007182).abs() < EPS,
        "first o at {}",
        first.time
    );

    // Absolute times must be monotonically non-decreasing across the stream.
    let mut prev = f64::NEG_INFINITY;
    for ev in &cast.events {
        assert!(
            ev.time >= prev - EPS,
            "time went backwards: {prev} -> {}",
            ev.time
        );
        prev = ev.time;
    }
}

#[test]
fn v2_sample_captures_resize_events() {
    let cast = Cast::parse(&read_sample_v2()).unwrap();
    let resizes: Vec<&str> = cast
        .events
        .iter()
        .filter(|e| e.code == EventCode::Resize)
        .map(|e| e.data.as_str())
        .collect();
    // The recording resizes 120x40 -> 200x40 -> 120x40.
    assert_eq!(resizes, vec!["200x40", "120x40"]);
}

#[test]
fn parses_synthetic_v3_header_from_term() {
    let cast = Cast::parse(&read_synthetic_v3()).expect("synthetic_v3 should parse");
    assert_eq!(cast.header.version, 3);
    // Dimensions come from term.cols / term.rows.
    assert_eq!(cast.header.width, 100);
    assert_eq!(cast.header.height, 30);
    // Theme is preserved from term.theme.
    let theme = cast.header.theme.expect("term.theme should be preserved");
    assert_eq!(theme["bg"], "#1a1b26");
}

#[test]
fn v3_delta_times_accumulate_exactly() {
    let cast = Cast::parse(&read_synthetic_v3()).unwrap();
    // Intervals 0.5, 0.25, 1.0, 0.75, 0.5 accumulate to these absolute times.
    let expected = [0.5_f64, 0.75, 1.75, 2.5, 3.0];
    assert_eq!(cast.events.len(), expected.len());
    for (ev, want) in cast.events.iter().zip(expected) {
        assert!((ev.time - want).abs() < EPS, "got {}, want {want}", ev.time);
    }

    // The comment line is skipped and the o/r/m codes land in order.
    assert_eq!(cast.events[0].code, EventCode::Output);
    assert_eq!(cast.events[2].code, EventCode::Resize);
    assert_eq!(cast.events[2].data, "120x40");
    assert_eq!(cast.events[3].code, EventCode::Marker);
    assert_eq!(cast.events[3].data, "chapter one");
}
