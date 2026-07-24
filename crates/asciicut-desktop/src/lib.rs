//! asciicut desktop shell — CASTCU-S2-T01 (shell) + CASTCU-S2-T02 (command
//! bridge) + CASTCU-S2-T03 (native file ops, app menu, dirty/quit guards).
//!
//! This crate opens the native OS window and renders the reused SolidJS SPA
//! (`web/`) inside the platform webview, loaded from the bundled
//! `frontendDist` in a release build (no terminal, no dev-server, no visible
//! `localhost`). It registers the [`commands`] `#[tauri::command]` handlers
//! over a mutex-guarded managed [`commands::SessionState`], so core editing
//! works with **no bound TCP port**. T03 adds a native File menu with
//! platform-standard shortcuts, OS open/save dialogs, and dirty-state/quit
//! guards coordinated with the SPA.

pub mod commands;
pub mod export;
pub mod sidecars;

use std::sync::Mutex;

use asciicut_bridge::Session;
use commands::{AppState, ExportState, QuitTarget};
use tauri::{Emitter, Manager};

/// Build a [`commands::SessionState`] from the process's launch arguments.
///
/// Mirrors the `asciicut` CLI's own bare `<file.cast>` launch convention
/// (`crates/asciicut/src/main.rs`): the first non-flag positional argument
/// ending in `.cast` is loaded into the session at startup (D4). A launch with
/// no such argument starts with an **empty** session — every command that
/// needs a loaded cast returns a clear "no cast loaded" bridge error (surfaced
/// to the frontend as a rejected `invoke()` promise) rather than panicking.
fn initial_session() -> Session {
    initial_session_from_args(std::env::args())
}

/// The testable core of [`initial_session`]: takes an argument iterator
/// (`std::env::args()` in production, a synthetic `Vec<String>` in tests) so
/// the launch-argument convention is unit-testable without spawning a real
/// process.
fn initial_session_from_args(args: impl Iterator<Item = String>) -> Session {
    let cast_arg = args.skip(1).find(|a| a.ends_with(".cast"));

    match cast_arg {
        Some(path) => {
            let mut session = Session::new();
            match session.load(path.clone().into()) {
                Ok(()) => session,
                Err(e) => {
                    // Non-fatal: log to stderr and start with an empty session
                    // rather than aborting the whole app over a bad launch
                    // argument — every bridge command surfaces "no cast
                    // loaded" until a valid cast is opened.
                    eprintln!("asciicut-desktop: cannot load '{path}': {e}");
                    Session::new()
                }
            }
        }
        None => Session::new(),
    }
}

/// Build and run the Tauri application.
///
/// The window (title, size, label) and the bundled frontend are configured
/// declaratively in `tauri.conf.json`; `generate_context!()` bakes that config
/// plus the SPA assets into the binary at compile time. The `mobile_entry_point`
/// attribute makes this the exported entry the mobile toolchains invoke.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(commands::SessionState(Mutex::new(initial_session())))
        .manage(commands::AppState::new())
        .manage(ExportState::new())
        .invoke_handler(tauri::generate_handler![
            commands::frame,
            commands::thumbs,
            commands::activity,
            commands::event_times,
            commands::compose_project,
            commands::save_project,
            commands::save_project_as,
            commands::project_meta,
            commands::open_cast,
            commands::open_cast_path,
            commands::open_sample,
            commands::cast_path,
            commands::set_dirty,
            commands::confirm_quit,
            commands::cancel_quit,
            commands::request_quit,
            commands::quit_now,
            commands::agg_version,
            commands::ffmpeg_version,
            commands::export_video,
            commands::cancel_export,
        ])
        .setup(|app| {
            use tauri::menu::Menu;

            let file_menu = commands::build_file_menu(app.handle())?;
            let menu = Menu::with_id_and_items(app.handle(), "app-menu", &[&file_menu])?;
            app.set_menu(menu)?;

            let handle = app.handle().clone();
            app.on_menu_event(move |_, event| {
                commands::handle_menu_event(&handle, event);
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::WindowEvent;

            if let WindowEvent::CloseRequested { api, .. } = event {
                if guard_dirty(&window.state::<AppState>(), window, QuitTarget::Close) {
                    // Block the close; the SPA now owns the save/discard choice.
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the asciicut desktop shell");

    app.run(|app_handle, event| {
        use tauri::RunEvent;

        if let RunEvent::ExitRequested { api, .. } = event {
            if guard_dirty(
                &app_handle.state::<AppState>(),
                app_handle,
                QuitTarget::Quit,
            ) {
                api.prevent_exit();
            }
        }
    });
}

/// The shared dirty guard behind both close and exit: if the SPA has unsaved
/// edits, claim the quit guard (once, however many attempts arrive), emit
/// `confirm-quit` so the SPA can offer save/discard/cancel, and report `true`
/// so the caller blocks its own close/exit. A clean model reports `false` and
/// the shutdown proceeds untouched.
fn guard_dirty(state: &AppState, emitter: &impl Emitter<tauri::Wry>, target: QuitTarget) -> bool {
    if !state.is_dirty() {
        return false;
    }
    if state.claim_quit_guard(target) {
        let payload = commands::ConfirmQuitPayload { target };
        let _ = emitter.emit("confirm-quit", &payload);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repo root = `<repo>/crates/asciicut-desktop` (`CARGO_MANIFEST_DIR`)
    /// popped two levels, mirroring `asciicut/tests/compose_cli.rs`'s helper.
    fn repo_root() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/
        p.pop(); // repo root
        p
    }

    #[test]
    fn no_cast_argument_starts_with_empty_session() {
        let args = vec!["asciicut-desktop".to_string()].into_iter();
        let session = initial_session_from_args(args);
        assert!(!session.is_loaded(), "no launch argument → empty session");
    }

    #[test]
    fn cast_argument_loads_the_session() {
        let sample = repo_root().join("samples/sample.cast");
        let args = vec![
            "asciicut-desktop".to_string(),
            sample.to_str().unwrap().to_string(),
        ]
        .into_iter();
        let session = initial_session_from_args(args);
        assert!(
            session.is_loaded(),
            "a <file.cast> launch argument loads the session"
        );
    }

    #[test]
    fn non_cast_argument_is_ignored() {
        // A bare flag/non-.cast token is not mistaken for a launch cast —
        // mirrors the CLI's `.ends_with(".cast")` gate (D4).
        let args = vec!["asciicut-desktop".to_string(), "--flag".to_string()].into_iter();
        let session = initial_session_from_args(args);
        assert!(!session.is_loaded());
    }

    #[test]
    fn missing_cast_file_falls_back_to_empty_session_not_panic() {
        let args = vec![
            "asciicut-desktop".to_string(),
            "/no/such/recording.cast".to_string(),
        ]
        .into_iter();
        let session = initial_session_from_args(args);
        assert!(
            !session.is_loaded(),
            "an unreadable launch cast must not panic — falls back to empty"
        );
    }
}
