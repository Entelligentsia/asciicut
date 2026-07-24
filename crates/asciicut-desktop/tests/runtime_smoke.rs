//! Tauri-runtime smoke coverage for the T03 dialog plugin registration and
//! quit-guard state machine. Uses Tauri v2's unstable `tauri::test` mock runtime
//! so the suite needs no display server.

use std::sync::atomic::Ordering;

use asciicut_desktop_lib::commands::{cancel_quit, request_quit, AppState, QuitTarget};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[test]
fn dialog_plugin_is_registered() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build test app");

    // Obtaining the dialog handle proves the plugin's init() managed Dialog<R>.
    let _dialog = app.dialog();
}

/// A cancelled save-on-quit prompt must clear `pending_quit` so the next
/// Quit/Close can re-arm the guard. Without this, the guard becomes inert after
/// the first cancellation.
#[test]
fn request_quit_cancel_resumes_guard() {
    let app = tauri::test::mock_builder()
        .manage(AppState::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build test app");

    let state = app.state::<AppState>();
    state.dirty.store(true, Ordering::Relaxed);
    request_quit(app.handle().clone(), state).expect("request_quit succeeds");

    let state = app.state::<AppState>();
    assert_eq!(
        *state.pending_quit.lock().expect("pending_quit poisoned"),
        Some(QuitTarget::Quit),
        "dirty request_quit arms the pending guard"
    );
    cancel_quit(state).expect("cancel_quit succeeds");

    let state = app.state::<AppState>();
    assert!(
        state
            .pending_quit
            .lock()
            .expect("pending_quit poisoned")
            .is_none(),
        "cancel_quit clears the pending guard"
    );

    let state = app.state::<AppState>();
    request_quit(app.handle().clone(), state).expect("second request_quit succeeds");

    let state = app.state::<AppState>();
    assert_eq!(
        *state.pending_quit.lock().expect("pending_quit poisoned"),
        Some(QuitTarget::Quit),
        "guard can be re-armed after a previous cancel"
    );
}
