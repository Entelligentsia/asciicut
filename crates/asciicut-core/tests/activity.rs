//! Integration tests for the activity-signal waveform over the real recording.
//!
//! These read the shipped v2 recording from disk (which the pure
//! `asciicut_core::activity` module never does) via `CARGO_MANIFEST_DIR`, parse it
//! through the real parser, and assert the waveform's shape end-to-end (AC#4).

use std::fs;
use std::path::PathBuf;

use asciicut_core::{activity_signal, Cast, DEFAULT_BUCKET_SECS};

/// Read the real v2 `samples/sample.cast` from the workspace root.
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
fn waveform_length_matches_duration_over_bucket() {
    let cast = Cast::parse(&read_sample_v2()).expect("sample.cast parses");
    let signal = activity_signal(&cast, DEFAULT_BUCKET_SECS);

    let duration = cast.events.last().map_or(0.0, |e| e.time);
    let expected = (duration / DEFAULT_BUCKET_SECS).ceil() as usize;

    assert_eq!(
        signal.len(),
        expected,
        "waveform length should equal ceil(duration / bucket_secs)"
    );
    assert!((signal.bucket_secs() - DEFAULT_BUCKET_SECS).abs() < 1e-12);
}

#[test]
fn all_scores_are_finite_and_non_negative() {
    let cast = Cast::parse(&read_sample_v2()).expect("sample.cast parses");
    let signal = activity_signal(&cast, DEFAULT_BUCKET_SECS);

    // Scores are `u64` byte counts — non-negative by type; assert the waveform
    // actually carries signal (the sample is a real, busy recording).
    let total: u64 = signal.buckets().iter().sum();
    assert!(
        total > 0,
        "a real recording must accumulate printable bytes"
    );
}

#[test]
fn waveform_is_non_degenerate_dead_air_is_visible() {
    let cast = Cast::parse(&read_sample_v2()).expect("sample.cast parses");
    let signal = activity_signal(&cast, DEFAULT_BUCKET_SECS);
    let buckets = signal.buckets();

    let peak = *buckets.iter().max().expect("non-empty waveform");
    assert!(peak > 0, "the busiest bucket must be non-zero");

    // Sample-relative thresholds (not absolute constants): a real recording has
    // both quiet valleys well below its peak and busy peaks well above its mean.
    let mean = buckets.iter().sum::<u64>() as f64 / buckets.len() as f64;

    let has_valley = buckets.iter().any(|&score| (score as f64) < mean * 0.5);
    let has_peak = buckets.iter().any(|&score| (score as f64) > mean * 1.5);

    assert!(
        has_valley,
        "expected at least one low-valley bucket below half the mean"
    );
    assert!(
        has_peak,
        "expected at least one high-peak bucket above 1.5x the mean"
    );
}
