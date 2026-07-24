//! AC#5 — the bridge's composed `.cast` is byte-identical to the CLI's compose
//! pipeline for the same source/project pair.
//!
//! `crates/asciicut/src/lib.rs::compose_project` (the CLI library function) is
//! itself nothing but `compose(&source_cast, &project).to_cast_string()` — the
//! CLI's own `cli_stdout_is_byte_identical_to_core_compose` test already pins
//! that. Depending on the `asciicut` crate directly from here would introduce a
//! package-graph cycle (`asciicut` → `asciicut-server` → `asciicut-bridge`, the
//! very crate whose tests would then depend back on `asciicut`), so this test
//! pins against the same underlying primitive `asciicut::compose_project`
//! calls: `asciicut_core::compose(...).to_cast_string()`. Because
//! `ops::compose_project` (below) calls that exact function, this is a
//! structural guarantee, not an incidental one (PLAN_REVIEW advisory) — but it
//! is pinned explicitly here per AC#5 rather than left implicit.

use std::path::PathBuf;

use asciicut_bridge::{ops, Session};
use asciicut_core::{compose, Cast, Project};

/// Repo root = `<repo>/crates/asciicut-bridge` (`CARGO_MANIFEST_DIR`) popped two
/// levels, mirroring `asciicut/tests/compose_cli.rs`'s own `repo_root` helper.
fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

fn read_repo(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// AC#5 — bridge compose == the same `compose().to_cast_string()` call the
/// CLI's `compose_project` and `/api/compose` both funnel through, for the S1
/// golden-cast fixture pair (`samples/sample.cast` + `samples/sample.asciicut.json`,
/// the same fixture `crates/asciicut/tests/compose_cli.rs` pins the CLI to).
#[test]
fn bridge_compose_is_byte_identical_to_core_compose() {
    let source_rel = "samples/sample.cast";
    let project_rel = "samples/sample.asciicut.json";

    let source_text = read_repo(source_rel);
    let source = Cast::parse(&source_text).expect("parse source cast");
    let project_body = read_repo(project_rel);

    // The bridge path: a Session seeded with the parsed source cast, then
    // `ops::compose_project` over the raw project body — exactly what an axum
    // `/api/compose` handler or a `#[tauri::command]` calls.
    let session = Session::with_cast(source, repo_root().join(source_rel));
    let bridge_output =
        ops::compose_project(&session, &project_body).expect("bridge compose succeeds");

    // The reference path: parse the same two fixtures independently and call
    // the core engine directly — the same call `asciicut::compose_project`
    // (crates/asciicut/src/lib.rs) makes.
    let source_again = Cast::parse(&source_text).expect("parse source cast (reference path)");
    let project = Project::parse(&project_body).expect("parse project (reference path)");
    let expected = compose(&source_again, &project).to_cast_string();

    assert_eq!(
        bridge_output, expected,
        "bridge compose output must be byte-identical to asciicut_core::compose(...).to_cast_string()"
    );
    assert!(
        !bridge_output.is_empty(),
        "sanity: the composed document must be non-empty"
    );
}

/// `Session::save_path` prefers an explicit project path when set, and falls
/// back to the sibling `<cast_stem>.asciicut.json` rule otherwise.
#[test]
fn session_save_path_prefers_explicit_project_path() {
    let source_rel = "samples/sample.cast";
    let source_text = read_repo(source_rel);
    let source = Cast::parse(&source_text).expect("parse source cast");
    let mut session = Session::with_cast(source, repo_root().join(source_rel));

    // Default: sibling rule.
    assert_eq!(
        session.save_path().unwrap(),
        repo_root().join("samples/sample.asciicut.json")
    );

    // Explicit path wins.
    let explicit = repo_root().join("samples/explicit.asciicut.json");
    session.set_project_path(explicit.clone());
    assert_eq!(session.save_path().unwrap(), explicit);

    // `load` clears the explicit path so the new cast reverts to sibling rule.
    session.load(repo_root().join(source_rel)).unwrap();
    assert_eq!(
        session.save_path().unwrap(),
        repo_root().join("samples/sample.asciicut.json")
    );
}

/// `ops::event_times` returns the cast's event timestamps ascending and with no
/// duplicate moment — the honest grid the editor snaps IN/OUT nudges to.
#[test]
fn event_times_are_ascending_deduped_and_match_the_source() {
    let source_rel = "samples/sample.cast";
    let source_text = read_repo(source_rel);
    let source = Cast::parse(&source_text).expect("parse source cast");
    let event_count = source.events.len();
    let last_time = source.events.last().map(|e| e.time);

    let session = Session::with_cast(source, repo_root().join(source_rel));
    let times = ops::event_times(&session).expect("event_times succeeds");

    assert!(!times.is_empty(), "the sample recording has events");
    // Strictly ascending — the de-dup guarantee (every entry a distinct moment).
    for pair in times.windows(2) {
        assert!(
            pair[0] < pair[1],
            "event times must be strictly ascending after de-dup: {pair:?}"
        );
    }
    // No moment is dropped or invented: the last event time still bounds the
    // list, and de-dup never grows it.
    assert!(times.len() <= event_count);
    assert_eq!(times.last().copied(), last_time);
}

/// An empty session has no cast, so `event_times` reports the same
/// `NoCastLoaded` error every other op does — the desktop shell's pre-open
/// state, not a panic.
#[test]
fn event_times_requires_a_loaded_cast() {
    let session = Session::new();
    assert!(matches!(
        ops::event_times(&session),
        Err(asciicut_bridge::BridgeError::NoCastLoaded)
    ));
}
