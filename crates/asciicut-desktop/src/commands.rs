//! `#[tauri::command]` handlers — one per `asciicut-bridge` operation plus
//! T03 native file operations and dirty/quit state (CASTCU-S2-T02/T03).
//!
//! Each command locks the managed [`SessionState`], calls the matching
//! `asciicut_bridge::ops` function directly (no re-implementation — this is the
//! *same* function `asciicut-server`'s axum routes call, AC#1), and maps a
//! [`asciicut_bridge::BridgeError`] to `Err(String)`: exactly how Tauri already
//! reports command failure to the frontend (`invoke()` rejects the promise
//! with that string). No new error plumbing beyond `.to_string()`.
//!
//! Registration lives in [`crate::run`] via `tauri::generate_handler!`. Tauri
//! v2's permission system governs *plugin*-provided commands; these
//! app-registered commands are invokable by the frontend without a
//! `capabilities/default.json` entry (D3). The `dialog` plugin is invoked from
//! Rust, so the SPA needs no dialog capability grant (least-privilege).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use asciicut_bridge::dto::{ActivitySignalDto, FrameDto, ProjectMeta, SaveResponse, ThumbDto};
use asciicut_bridge::{ops, Session};
use serde::Serialize;
use tauri::menu::MenuItem;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::process::CommandChild;

use crate::export::{self, ExportFormat, ExportResult};

/// The managed application session: a mutex-guarded [`Session`] so a "open a
/// different cast" can swap the loaded cast without restarting the app.
pub struct SessionState(pub Mutex<Session>);

/// Managed desktop-shell state that the SPA reports to and coordinates with.
pub struct AppState {
    /// Whether the SPA has unsaved edits.
    pub dirty: AtomicBool,
    /// If a close/exit was prevented because the model was dirty, this holds
    /// the action to complete once the SPA confirms (or saves then confirms).
    pub pending_quit: Mutex<Option<QuitTarget>>,
}

/// What to do after the SPA confirms the quit/close guard.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum QuitTarget {
    /// Close the main window (window close button / Cmd+W / similar).
    Close,
    /// Exit the process (Cmd+Q / File → Quit / app shutdown).
    Quit,
}

/// Payload emitted on `confirm-quit` when the shell blocks close/exit because
/// the editor has unsaved edits.
#[derive(Debug, Clone, Serialize)]
pub struct ConfirmQuitPayload {
    pub target: QuitTarget,
}

impl AppState {
    /// Build a fresh app state with no dirty flag and no pending quit action.
    #[must_use]
    pub fn new() -> Self {
        Self {
            dirty: AtomicBool::new(false),
            pending_quit: Mutex::new(None),
        }
    }

    /// Whether the SPA currently reports unsaved edits.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Claim the quit guard for `target`, returning whether THIS call claimed
    /// it (so exactly one `confirm-quit` is emitted no matter how many
    /// close/exit attempts pile up). A guard already pending is left as-is.
    ///
    /// One lock for the check and the store — a `is_none()` probe followed by a
    /// separate store would leave a window where two attempts both see "free".
    #[must_use]
    pub fn claim_quit_guard(&self, target: QuitTarget) -> bool {
        let mut pending = self.pending_quit.lock().expect("pending_quit poisoned");
        if pending.is_some() {
            return false;
        }
        *pending = Some(target);
        true
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Managed state for the export pipeline (CASTCU-S2-T05): the currently
/// tracked sidecar child process (if any export is mid-flight) plus a cancel
/// flag `export::run_export`'s spawn loop consults after every child exit —
/// whether the exit was the process finishing on its own or `cancel_export`
/// killing it.
///
/// A single `Mutex<Option<CommandChild>>` intentionally supports only one
/// in-flight export at a time; the SPA's Render button is disabled while an
/// export is running (advisory from plan review: concurrent `export_video`
/// calls would otherwise race on this tracked child).
pub struct ExportState {
    child: Mutex<Option<CommandChild>>,
    cancelled: AtomicBool,
}

impl ExportState {
    /// Build fresh export state — no child tracked, not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Reset the cancel flag for a new export run. Must be called before a
    /// fresh `run_export` so a prior export's cancellation does not leak into
    /// the next one.
    pub(crate) fn begin(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    /// Whether the currently (or most recently) tracked export was cancelled.
    #[must_use]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Track a freshly spawned sidecar child so `cancel` can kill it.
    pub(crate) fn track(&self, child: CommandChild) {
        let mut guard = self.child.lock().expect("export child mutex poisoned");
        *guard = Some(child);
    }

    /// Stop tracking the current child (it has terminated on its own, or was
    /// just taken by `cancel`).
    pub(crate) fn untrack(&self) {
        let mut guard = self.child.lock().expect("export child mutex poisoned");
        *guard = None;
    }

    /// Set the cancel flag and kill the currently tracked child, if any. A
    /// no-op beyond setting the flag when no child is tracked yet (e.g. the
    /// pipeline is still in its write stage) — the next spawn stage checks
    /// `is_cancelled` before starting and bails out instead.
    ///
    /// `pub` (not `pub(crate)`, unlike this struct's other internals) because
    /// `tests/export_runtime_smoke.rs` calls it directly from a background
    /// thread to exercise real mid-pipeline cancellation against
    /// `export::run_export` without driving a native save dialog headlessly.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let child = {
            let mut guard = self.child.lock().expect("export child mutex poisoned");
            guard.take()
        };
        if let Some(child) = child {
            let _ = child.kill();
        }
    }
}

impl Default for ExportState {
    fn default() -> Self {
        Self::new()
    }
}

/// A native save dialog's default file name: `path`'s own final component, or
/// `fallback` when there is no path (no cast loaded yet) or no final component.
fn file_name_or(path: &Option<PathBuf>, fallback: &str) -> String {
    path.as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(fallback)
        .to_string()
}

/// A native save dialog's initial directory: `path`'s parent, or an empty path
/// (which the dialog reads as "no preference") when there is none.
fn parent_or_empty(path: &Option<PathBuf>) -> PathBuf {
    path.as_ref()
        .and_then(|p| p.parent())
        .map_or_else(PathBuf::new, PathBuf::from)
}

/// `frame` — the styled screen grid at source time `t`. Mirrors `GET
/// /api/frame?t=<seconds>`.
#[tauri::command]
pub fn frame(state: State<SessionState>, t: f64) -> Result<FrameDto, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::frame(&session, t).map_err(|e| e.to_string())
}

/// `thumbs` — `count` evenly-spaced filmstrip frames. Mirrors `GET
/// /api/thumbs?count=<n>`.
#[tauri::command]
pub fn thumbs(state: State<SessionState>, count: Option<usize>) -> Result<Vec<ThumbDto>, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::thumbs(&session, count).map_err(|e| e.to_string())
}

/// `activity` — the change-density waveform. Mirrors `GET
/// /api/activity?bucket=<seconds>`.
#[tauri::command]
pub fn activity(
    state: State<SessionState>,
    bucket: Option<f64>,
) -> Result<ActivitySignalDto, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::activity(&session, bucket).map_err(|e| e.to_string())
}

/// `event_times` — the loaded cast's real event timestamps, ascending and
/// de-duplicated. Mirrors `GET /api/events`. The editor snaps IN/OUT nudges to
/// these, so a nudged boundary always lands on a frame where something changed.
#[tauri::command]
pub fn event_times(state: State<SessionState>) -> Result<Vec<f64>, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::event_times(&session).map_err(|e| e.to_string())
}

/// `compose_project` — compose an edit project against the loaded source
/// cast, returning the composed `.cast` document as text. Mirrors `POST
/// /api/compose`.
#[tauri::command]
pub fn compose_project(state: State<SessionState>, project: String) -> Result<String, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::compose_project(&session, &project).map_err(|e| e.to_string())
}

/// `save_project` — persist an edit project to the active save path. Mirrors
/// `POST /api/save`.
#[tauri::command]
pub fn save_project(state: State<SessionState>, project: String) -> Result<SaveResponse, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::save_project(&session, &project).map_err(|e| e.to_string())
}

/// `save_project_as` — show a native save dialog, write the project to the
/// chosen location, and make that location the new active save path. Returns
/// the path so the SPA can reflect it in the title and subsequent Save calls.
#[tauri::command]
pub async fn save_project_as(
    app: AppHandle,
    state: State<'_, SessionState>,
    project: String,
) -> Result<SaveResponse, String> {
    use tauri_plugin_dialog::DialogExt;

    let dialog_path: Option<PathBuf> = {
        let session = state.0.lock().expect("session mutex poisoned");
        // Seed the dialog from the session's own save target, so Save As
        // defaults to the file Save would write (the sibling `.asciicut.json`,
        // or whatever a previous Save As established) rather than a
        // hand-rebuilt name.
        let save_path = session.save_path().ok();
        let default_name = file_name_or(&save_path, "project.asciicut.json");
        let initial_dir = parent_or_empty(&save_path);

        // Drop the lock before the dialog runs so the UI stays responsive.
        drop(session);

        app.dialog()
            .file()
            .set_title("Save project")
            .set_directory(initial_dir)
            .set_file_name(default_name)
            .add_filter("asciicut project", &["asciicut.json"])
            .blocking_save_file()
            .and_then(|fp| fp.into_path().ok())
    };

    let Some(path) = dialog_path else {
        // Cancellation is a silent no-op.
        return Err("cancelled".to_string());
    };

    let mut session = state.0.lock().expect("session mutex poisoned");
    session.set_project_path(path.clone());
    ops::save_project_to(&session, &project, path).map_err(|e| e.to_string())
}

/// `export_video` — show a native save dialog scoped to the chosen format,
/// then run the full export pipeline: write the round-trip triple (the
/// session-derived `.asciicut.json` plus the desktop-local `.composed.cast`),
/// render it through the bundled `agg` sidecar to a GIF, and (for `mp4`/
/// `webm`) transcode that GIF through the bundled `ffmpeg` sidecar into the
/// chosen container. Streams `export-progress` events throughout; honors
/// cancellation via `cancel_export`.
///
/// `format` is one of `"mp4" | "webm" | "gif"`. `cut_duration_secs` is the
/// SPA's own already-computed cut duration (`cutTimeLabel()`'s source value),
/// used only to scale `ffmpeg`'s fine-grained progress percentage — never
/// trusted for anything else.
///
/// Cancelling the save dialog is a silent no-op (`Err("cancelled")`), the same
/// convention `open_cast`/`save_project_as` use; nothing is written or
/// spawned. A cancellation via `cancel_export` mid-pipeline surfaces the same
/// `Err("cancelled")` string from a different code path — both are non-error
/// user actions, not failures, so the SPA can treat the one string uniformly.
#[tauri::command]
pub async fn export_video<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, SessionState>,
    export_state: State<'_, ExportState>,
    project: String,
    format: String,
    cut_duration_secs: f64,
) -> Result<ExportResult, String> {
    use tauri_plugin_dialog::DialogExt;

    let format = ExportFormat::parse(&format)?;

    let dialog_path: Option<PathBuf> = {
        let session = state.0.lock().expect("session mutex poisoned");
        let source_name = session.source_name().map_err(|e| e.to_string())?;
        let default_name = export::default_output_name(&source_name, format);
        let initial_dir = parent_or_empty(&session.cast_path().ok().map(PathBuf::from));

        // Drop the lock before the dialog runs so the UI stays responsive.
        drop(session);

        app.dialog()
            .file()
            .set_title("Export video")
            .set_directory(initial_dir)
            .set_file_name(default_name)
            .add_filter(format.extension(), &[format.extension()])
            .blocking_save_file()
            .and_then(|fp| fp.into_path().ok())
    };

    let Some(video_path) = dialog_path else {
        // Cancellation is a silent no-op.
        return Err("cancelled".to_string());
    };

    // Gather everything the (non-blocking-lock) pipeline needs from the
    // session up front, then drop the lock — `run_export` awaits sidecar
    // processes and must never hold a `std::sync::Mutex` guard across an
    // `.await`.
    let (project_path, composed_text, cast_export_path) = {
        let session = state.0.lock().expect("session mutex poisoned");
        let save_response = ops::save_project(&session, &project).map_err(|e| e.to_string())?;
        let composed_text = ops::compose_project(&session, &project).map_err(|e| e.to_string())?;
        let cast_path = session.cast_path().map_err(|e| e.to_string())?;
        let cast_export_path = export::export_cast_path(cast_path);
        (save_response.path, composed_text, cast_export_path)
    };

    export::run_export(
        &app,
        &export_state,
        format,
        cut_duration_secs,
        video_path,
        project_path,
        composed_text,
        cast_export_path,
    )
    .await
}

/// `cancel_export` — kill the currently tracked `agg`/`ffmpeg` sidecar child
/// (if any) and mark the in-flight export cancelled, so its terminal
/// `export-progress` event and `export_video`'s own `Result` both report
/// `cancelled` rather than a spurious process-killed error. A silent no-op if
/// no export is currently running.
#[tauri::command]
pub fn cancel_export(export_state: State<ExportState>) -> Result<(), String> {
    export_state.cancel();
    Ok(())
}

/// `project_meta` — the loaded cast's authoritative source name plus any
/// persisted project. Mirrors `GET /api/project`.
#[tauri::command]
pub fn project_meta(state: State<SessionState>) -> Result<ProjectMeta, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    ops::project_meta(&session).map_err(|e| e.to_string())
}

/// `open_cast` — show a native `.cast` file picker, load the chosen recording
/// into the managed session, and emit a `cast-opened` event carrying the
/// authoritative source name and sibling project. Returns the same payload.
#[tauri::command]
pub async fn open_cast(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<OpenedCast, String> {
    use tauri_plugin_dialog::DialogExt;

    let dialog_path: Option<PathBuf> = app
        .dialog()
        .file()
        .set_title("Open cast recording")
        .add_filter("asciinema cast", &["cast"])
        .blocking_pick_file()
        .and_then(|fp| fp.into_path().ok());

    let Some(path) = dialog_path else {
        // Cancellation is a silent no-op.
        return Err("cancelled".to_string());
    };

    load_and_announce(&app, &state, path)
}

/// Load `path` into the managed session and announce it, the shared tail of
/// every "open a cast" path that is *not* the native picker (`open_cast_path`
/// for a Recent entry, `open_sample` for the bundled recording).
fn load_and_announce(
    app: &AppHandle,
    state: &SessionState,
    path: PathBuf,
) -> Result<OpenedCast, String> {
    let mut session = state.0.lock().expect("session mutex poisoned");
    session.load(path.clone()).map_err(|e| e.to_string())?;
    let meta = ops::project_meta(&session).map_err(|e| e.to_string())?;
    // Drop the lock before emitting: listeners may re-enter a command.
    drop(session);

    let opened = OpenedCast {
        path: path.display().to_string(),
        meta,
    };
    app.emit("cast-opened", &opened)
        .map_err(|e| format!("failed to emit cast-opened: {e}"))?;

    Ok(opened)
}

/// The payload every "a cast is now loaded" path produces — the return value of
/// [`open_cast`] / [`open_cast_path`] / [`open_sample`] *and* the body of the
/// `cast-opened` event.
///
/// It is [`ProjectMeta`] plus the resolved absolute `path`. The path is what
/// lets the welcome screen's **Recent** list re-open a recording later, so it
/// has to travel with every open — including the native picker's, whose chosen
/// path the SPA otherwise never sees.
#[derive(Debug, Serialize)]
pub struct OpenedCast {
    /// Absolute path the cast was loaded from.
    pub path: String,
    /// The authoritative source name plus any persisted sibling project.
    pub meta: ProjectMeta,
}

/// `cast_path` — the absolute path of the currently loaded cast, or `null` when
/// the shell launched with no recording. The startup counterpart of
/// [`OpenedCast::path`]: it lets the SPA record a launch-argument cast in
/// **Recent** without re-opening it.
#[tauri::command]
pub fn cast_path(state: State<SessionState>) -> Result<Option<String>, String> {
    let session = state.0.lock().expect("session mutex poisoned");
    Ok(session.cast_path().ok().map(|p| p.display().to_string()))
}

/// `open_cast_path` — load a cast from an explicit path, with no dialog. This
/// backs the welcome screen's **Recent** list.
///
/// The path is caller-supplied, so provenance matters: the SPA only ever passes
/// back a path it previously received from [`open_cast`]'s native picker (or
/// from [`open_sample`]) and persisted. This is deliberately *not* treated as
/// untrusted input needing a sandbox — the webview renders only bundled local
/// content, and the same user can already reach any file through the picker —
/// but it is also deliberately NOT a write target: `save_path` keeps deriving
/// its own sibling, so opening a path can never redirect a later save.
///
/// A missing or unparseable file surfaces the bridge's own diagnostic, which is
/// what lets the SPA prune a stale Recent entry with an honest message.
#[tauri::command]
pub async fn open_cast_path(
    app: AppHandle,
    state: State<'_, SessionState>,
    path: String,
) -> Result<OpenedCast, String> {
    load_and_announce(&app, &state, PathBuf::from(path))
}

/// `open_sample` — load the bundled `sample.cast` (the welcome screen's "Try
/// the sample"), producing the same [`OpenedCast`] result and `cast-opened`
/// event every other open path does.
///
/// Resolution prefers the packaged resource; in a `tauri dev` tree that
/// resource may not be staged yet, so it falls back to the repo's own
/// `samples/sample.cast` (a compile-time path, not a runtime search).
#[tauri::command]
pub async fn open_sample(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> Result<OpenedCast, String> {
    let path = sample_cast_path(&app)?;
    load_and_announce(&app, &state, path)
}

/// The bundled sample recording's path: the packaged resource if it is there,
/// otherwise the repo copy this binary was built from.
fn sample_cast_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resource) = app.path().resolve("sample.cast", BaseDirectory::Resource) {
        if resource.is_file() {
            return Ok(resource);
        }
    }
    let dev = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../samples/sample.cast"
    ));
    if dev.is_file() {
        return Ok(dev);
    }
    Err("bundled sample.cast not found".to_string())
}

/// `set_dirty` — reported by the SPA whenever the edit model becomes dirty or
/// clean.
#[tauri::command]
pub fn set_dirty(state: State<AppState>, dirty: bool) -> Result<(), String> {
    state.dirty.store(dirty, Ordering::Relaxed);
    Ok(())
}

/// `confirm_quit` — complete a quit/close that was previously blocked by the
/// dirty guard. Clears the dirty flag and pending action, then closes the
/// window or exits the process.
#[tauri::command]
pub fn confirm_quit<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<AppState>,
) -> Result<(), String> {
    let target = {
        let mut pending = state.pending_quit.lock().expect("pending_quit poisoned");
        pending.take()
    };

    let Some(target) = target else {
        // No pending guard — nothing to do.
        return Ok(());
    };

    // The SPA has saved or explicitly chosen to discard changes; clear dirty.
    state.dirty.store(false, Ordering::Relaxed);

    match target {
        QuitTarget::Close => {
            if let Some(window) = app.get_webview_window("main") {
                window.close().map_err(|e| e.to_string())?;
            }
        }
        QuitTarget::Quit => {
            app.exit(0);
        }
    }

    Ok(())
}

/// `request_quit` — initiate File → Quit with the same dirty guard used for
/// the window-close / app-shutdown paths. If the model is dirty, sets a pending
/// quit target and asks the SPA to confirm/save; otherwise exits immediately.
#[tauri::command]
pub fn request_quit<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<AppState>,
) -> Result<(), String> {
    if !state.is_dirty() {
        app.exit(0);
        return Ok(());
    }
    if state.claim_quit_guard(QuitTarget::Quit) {
        let payload = ConfirmQuitPayload {
            target: QuitTarget::Quit,
        };
        app.emit("confirm-quit", &payload)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `quit_now` — immediately terminate the process. Used for a clean File →
/// Quit when the SPA has already confirmed there are no unsaved edits.
#[tauri::command]
pub fn quit_now<R: tauri::Runtime>(app: AppHandle<R>) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

/// `agg_version` — prove the bundled `agg` sidecar invoke path by running
/// `agg --version` through `tauri-plugin-shell` (CASTCU-S2-T04). Returns the
/// first line of stdout so the frontend can display it as a health check.
#[tauri::command]
pub async fn agg_version<R: tauri::Runtime>(app: AppHandle<R>) -> Result<String, String> {
    run_sidecar_version(app, crate::sidecars::Sidecar::Agg).await
}

/// `ffmpeg_version` — prove the bundled `ffmpeg` sidecar invoke path by running
/// `ffmpeg -version` through `tauri-plugin-shell` (CASTCU-S2-T04).
#[tauri::command]
pub async fn ffmpeg_version<R: tauri::Runtime>(app: AppHandle<R>) -> Result<String, String> {
    run_sidecar_version(app, crate::sidecars::Sidecar::Ffmpeg).await
}

/// Shared implementation for the sidecar `--version` smoke commands.
///
/// 1. Resolve the sidecar next to the running app binary and verify it
///    exists (no PATH fallback) — the same directory `Command::sidecar`
///    itself resolves against (CASTCU-S2-T06; see `sidecars.rs`'s module
///    doc for why this is the exe dir, not `app.path().resource_dir()`).
/// 2. Spawn it via `tauri-plugin-shell` so Tauri v2 ACL governs the process.
/// 3. Capture stdout and return the first non-empty line.
async fn run_sidecar_version<R: tauri::Runtime>(
    app: AppHandle<R>,
    sidecar: crate::sidecars::Sidecar,
) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;

    let exe_dir = crate::sidecars::current_exe_dir().map_err(|e| e.to_string())?;
    crate::sidecars::resolve_checked(&exe_dir, sidecar).map_err(|e| e.to_string())?;

    let arg = match sidecar {
        crate::sidecars::Sidecar::Agg => "--version",
        crate::sidecars::Sidecar::Ffmpeg => "-version",
    };

    let output = app
        .shell()
        .sidecar(sidecar.name())
        .map_err(|e| format!("failed to prepare {} sidecar: {e}", sidecar.name()))?
        .arg(arg)
        .output()
        .await
        .map_err(|e| format!("failed to spawn {}: {e}", sidecar.name()))?;

    if !output.status.success() {
        return Err(format!(
            "{} exited with status {}",
            sidecar.name(),
            output.status.code().unwrap_or(-1)
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let first_line = text.lines().next().unwrap_or("").trim().to_string();
    if first_line.is_empty() {
        return Err(format!("{} produced no output", sidecar.name()));
    }
    Ok(first_line)
}

/// `cancel_quit` — clear a pending quit/close guard without completing it.
/// Called by the SPA when the user cancels the save-on-quit prompt (or a save
/// fails) so the guard stays live for the next Quit/Close attempt.
#[tauri::command]
pub fn cancel_quit(state: State<AppState>) -> Result<(), String> {
    let mut pending = state.pending_quit.lock().expect("pending_quit poisoned");
    *pending = None;
    Ok(())
}

/// Menu item ids used by the native File menu. The `id()` of each item is the
/// string that reaches `on_menu_event`.
pub mod menu_ids {
    pub const OPEN: &str = "file-open";
    pub const SAVE: &str = "file-save";
    pub const SAVE_AS: &str = "file-save-as";
    pub const EXPORT: &str = "file-export";
    pub const QUIT: &str = "file-quit";
}

/// Build the native File menu: Open, Save, Save As, Export, Quit, with
/// platform-standard accelerators.
pub fn build_file_menu<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::menu::Submenu<R>, tauri::Error> {
    use tauri::menu::Submenu;

    let open = MenuItem::with_id(app, menu_ids::OPEN, "Open", true, Some("CmdOrCtrl+O"))?;
    let save = MenuItem::with_id(app, menu_ids::SAVE, "Save", true, Some("CmdOrCtrl+S"))?;
    let save_as = MenuItem::with_id(
        app,
        menu_ids::SAVE_AS,
        "Save As…",
        true,
        Some("CmdOrCtrl+Shift+S"),
    )?;
    let export = MenuItem::with_id(app, menu_ids::EXPORT, "Export", true, Some("CmdOrCtrl+E"))?;
    let quit = MenuItem::with_id(app, menu_ids::QUIT, "Quit", true, Some("CmdOrCtrl+Q"))?;

    let file = Submenu::with_id_and_items(
        app,
        "file",
        "File",
        true,
        &[&open, &save, &save_as, &export, &quit],
    )?;
    Ok(file)
}

/// Map a native File menu event to a frontend event so the SPA remains the
/// source of truth for the action.
pub fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let event_name = match event.id().0.as_str() {
        menu_ids::OPEN => "menu:open",
        menu_ids::SAVE => "menu:save",
        menu_ids::SAVE_AS => "menu:save-as",
        menu_ids::EXPORT => "menu:export",
        menu_ids::QUIT => "menu:quit",
        // A menu item this build does not handle (none today).
        _ => return,
    };
    let _ = app.emit(event_name, ());
}
