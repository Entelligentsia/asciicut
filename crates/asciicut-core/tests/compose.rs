//! Integration tests for the compose engine over on-disk fixtures.
//!
//! These read real files from disk (which the pure `asciicut_core::compose` module
//! never does) via `CARGO_MANIFEST_DIR`: the real v2 recording shipped at the
//! repo root (`samples/sample.cast` + `samples/sample.asciicut.json`), the golden
//! composed reference captured from `prototype/compose.py`, and a tiny synthetic
//! source + project with exact, hand-computable timings. See
//! `tests/fixtures/compose/README.md` for the fixture regeneration commands.

use std::fs;
use std::path::PathBuf;

use asciicut_core::{compose, Cast, EventCode, Project};

/// Epsilon for float time comparisons — never compare `f64` with `==`.
const EPS: f64 = 1e-6;

/// Read a path relative to the repo (workspace) root.
fn read_repo(rel: &str) -> String {
    // CARGO_MANIFEST_DIR is `<repo>/crates/asciicut-core`; the repo root is two
    // levels up.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // repo root
    path.push(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Read a fixture next to this test file (`tests/fixtures/compose/<name>`).
fn read_fixture(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/compose");
    path.push(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Compose the real 17-minute sample end-to-end and diff it, **semantically**,
/// against the prototype's golden composed output.
///
/// This is the AC#2/AC#3 gate: the Rust port must reproduce the prototype's own
/// composition of `samples/sample.cast`. Comparison is header equality + event
/// count + byte-exact code/data + epsilon-equal times (see the fixture README for
/// why byte-for-byte float text is deliberately not compared).
#[test]
fn composes_sample_to_golden_reference() {
    let source = Cast::parse(&read_repo("samples/sample.cast")).expect("sample.cast parses");
    let project =
        Project::parse(&read_repo("samples/sample.asciicut.json")).expect("project parses");

    let composed = compose(&source, &project);
    let reference = Cast::parse(&read_fixture("sample.composed.cast")).expect("golden parses");

    // Header equality (incl. width/height override, timestamp, env, version=2).
    assert_eq!(
        composed.header, reference.header,
        "composed header mismatch"
    );
    assert_eq!(composed.header.version, 2, "composed output must be v2");
    assert_eq!(composed.header.width, 120);
    assert_eq!(composed.header.height, 40);

    // Identical event count.
    assert_eq!(
        composed.events.len(),
        reference.events.len(),
        "composed event count diverged from the golden reference"
    );

    // Each event: byte-exact code + data, epsilon-equal time.
    for (i, (got, want)) in composed
        .events
        .iter()
        .zip(reference.events.iter())
        .enumerate()
    {
        assert_eq!(got.code, want.code, "event {i} code mismatch");
        assert_eq!(got.data, want.data, "event {i} data mismatch");
        assert!(
            (got.time - want.time).abs() < EPS,
            "event {i} time: got {}, want {}",
            got.time,
            want.time
        );
    }

    // The whole composed stream is output-only (prototype emits only `o`).
    assert!(composed.events.iter().all(|e| e.code == EventCode::Output));
}

/// The synthetic source + project pins the arithmetic against exact, known
/// absolute times, independent of the large sample.
#[test]
fn synthetic_fixture_has_exact_timings() {
    let source = Cast::parse(&read_fixture("synthetic.cast")).expect("synthetic.cast parses");
    let project =
        Project::parse(&read_fixture("synthetic.asciicut.json")).expect("synthetic project parses");

    let composed = compose(&source, &project);

    // Hand-computed (idleCap 0.4): seg0 [0,1] emits a@0.0, b@0.2, c@0.6 (0.8 gap
    // clamped to 0.4); seg1 [2,2] adds a 0.5 BEAT then d@1.1, then a 1.0 end-hold
    // tick at 2.1. Matches `python3 prototype/compose.py synthetic.asciicut.json`.
    let expected: [(f64, &str); 5] = [(0.0, "a"), (0.2, "b"), (0.6, "c"), (1.1, "d"), (2.1, "")];
    assert_eq!(composed.events.len(), expected.len());
    for (ev, (time, data)) in composed.events.iter().zip(expected) {
        assert_eq!(ev.code, EventCode::Output);
        assert_eq!(ev.data, data);
        assert!((ev.time - time).abs() < EPS, "got {}, want {time}", ev.time);
    }
}

/// The composed sample serializes back to a v2 `.cast` and round-trips through
/// the parser to an equal model — exercising the serializer on real output.
#[test]
fn composed_sample_serializes_and_round_trips() {
    let source = Cast::parse(&read_repo("samples/sample.cast")).unwrap();
    let project = Project::parse(&read_repo("samples/sample.asciicut.json")).unwrap();
    let composed = compose(&source, &project);

    let text = composed.to_cast_string();
    let reparsed = Cast::parse(&text).expect("serialized compose output re-parses");

    assert_eq!(reparsed.header.version, 2);
    assert_eq!(
        reparsed, composed,
        "compose -> serialize -> parse must be identity"
    );
}
