//! Desktop-scoped smoke coverage (secondary — see this crate's manifest-path
//! fmt/clippy/build/test convention, T01's documented coverage-gap note).
//!
//! Exercises the exact `asciicut_bridge::ops` functions `crates/asciicut-desktop
//! /src/commands.rs`'s `#[tauri::command]` handlers delegate to (they are
//! "thin wrappers only" — no re-implementation, AC#1), against the S1
//! golden-cast fixture pair. This proves the desktop transport's operational
//! logic end-to-end at the bridge boundary. It does NOT drive the handlers
//! through the actual Tauri `invoke()`/`State` runtime machinery — that full
//! round trip (launch → activity/thumbs/frame → edit → compose → save, with
//! zero bound listening sockets) is AC#4's Xvfb-driven smoke against the real
//! built binary, not a unit-style test here.

use std::path::PathBuf;

use asciicut_bridge::{ops, Session};

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

/// A unique scratch directory under the OS temp dir, removed on `Drop` —
/// mirrors `asciicut/tests/compose_cli.rs`'s own helper, so `save_project`
/// writes somewhere disposable rather than the repo tree.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "asciicut-desktop-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `Session` seeded from a copy of the S1 golden-cast fixture living inside
/// a disposable temp dir, so `save_project`/`project_meta` exercise a real
/// session-derived path without touching the repo tree.
fn fixture_session(tmp: &TempDir) -> Session {
    let cast_path = tmp.0.join("sample.cast");
    std::fs::copy(repo_root().join("samples/sample.cast"), &cast_path)
        .expect("copy fixture cast into temp dir");
    let mut session = Session::new();
    session.load(cast_path).expect("load fixture cast");
    session
}

/// `save_project_to` writes to a caller-supplied explicit path and does not
/// alter the session's default sibling-derived path.
#[test]
fn save_project_to_writes_to_explicit_path() {
    let tmp = TempDir::new("save-as");
    let session = fixture_session(&tmp);
    let project_body = read_repo("samples/sample.asciicut.json");

    let explicit = tmp.0.join("moved.asciicut.json");
    let saved = ops::save_project_to(&session, &project_body, explicit.clone())
        .expect("save_project_to succeeds");
    assert_eq!(saved.path, explicit.display().to_string());
    assert_eq!(saved.bytes, project_body.len());
    assert!(explicit.exists(), "explicit file was written");

    // Session save path is still the sibling rule, because `save_project_to`
    // does not set `project_path` — that is the desktop command's job.
    assert_eq!(
        session.save_path().unwrap(),
        tmp.0.join("sample.asciicut.json")
    );
}

/// AC#1 — `Session::set_project_path` makes `save_path` prefer the explicit
/// location, so Save rewrites the active Save As target.
#[test]
fn session_project_path_becomes_active_save_target() {
    let tmp = TempDir::new("active-save");
    let mut session = fixture_session(&tmp);
    let project_body = read_repo("samples/sample.asciicut.json");

    let explicit = tmp.0.join("active.asciicut.json");
    session.set_project_path(explicit.clone());
    let saved = ops::save_project(&session, &project_body).expect("save rewrites active path");
    assert_eq!(saved.path, explicit.display().to_string());
    assert!(explicit.exists());
}

/// The desktop `AppState` dirty flag is the primitive the SPA reports to and
/// the close/exit guards read.
#[test]
fn app_state_dirty_flag_lifecycle() {
    use std::sync::atomic::Ordering;

    let state = asciicut_desktop_lib::commands::AppState::new();
    assert!(!state.dirty.load(Ordering::Relaxed));

    state.dirty.store(true, Ordering::Relaxed);
    assert!(state.dirty.load(Ordering::Relaxed));

    state.dirty.store(false, Ordering::Relaxed);
    assert!(!state.dirty.load(Ordering::Relaxed));
    assert!(state.pending_quit.lock().unwrap().is_none());
}

/// Full round trip: activity → thumbs → frame → compose → save → project,
/// each call going through the exact bridge op the desktop commands wrap.
#[test]
fn full_bridge_round_trip_against_fixture_cast() {
    let tmp = TempDir::new("round-trip");
    let session = fixture_session(&tmp);
    let project_body = read_repo("samples/sample.asciicut.json");

    // activity
    let activity = ops::activity(&session, None).expect("activity succeeds");
    assert!(
        !activity.buckets.is_empty(),
        "fixture cast has a non-empty waveform"
    );

    // thumbs
    let thumbs = ops::thumbs(&session, Some(4)).expect("thumbs succeeds");
    assert_eq!(thumbs.len(), 4);

    // frame
    let frame = ops::frame(&session, 30.0).expect("frame succeeds");
    assert!(frame.width > 0 && frame.height > 0);

    // compose
    let composed = ops::compose_project(&session, &project_body).expect("compose succeeds");
    assert!(
        composed.lines().count() >= 2,
        "composed doc has a header plus at least one event line"
    );

    // save
    let saved = ops::save_project(&session, &project_body).expect("save succeeds");
    assert_eq!(saved.bytes, project_body.len());

    // project — the just-saved project is echoed back
    let meta = ops::project_meta(&session).expect("project_meta succeeds");
    assert_eq!(meta.source, "sample.cast");
    assert!(meta.project.is_some(), "the saved project is echoed back");
}

/// A `Session` with no cast loaded surfaces `NoCastLoaded` from every op that
/// needs one, rather than panicking — the desktop-only failure mode `asciicut-
/// server`'s `AppState` never hits (it always has a cast loaded).
#[test]
fn empty_session_reports_no_cast_loaded_not_panic() {
    let session = Session::new();
    let err = ops::frame(&session, 0.0).expect_err("no cast loaded is an error, not a panic");
    assert!(matches!(err, asciicut_bridge::BridgeError::NoCastLoaded));
}
