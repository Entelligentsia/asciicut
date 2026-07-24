//! Desktop-only video export pipeline (CASTCU-S2-T05): compose the loaded
//! project against the source cast into a `.composed.cast` (byte-identical to
//! `/api/compose`/the CLI, AC#8), then render it through the bundled `agg` →
//! `ffmpeg` sidecars (CASTCU-S2-T04) into `.mp4`/`.webm`/`.gif`.
//!
//! Pipeline shape locked by `SPEC.md` §9 (see PLAN.md's Approach): `agg`
//! always produces the GIF (its only output format); a `.gif` export stops
//! there, `.mp4`/`.webm` additionally transcode that GIF through `ffmpeg`.
//! Neither sidecar receives an explicit `--theme`/`--font-family` override —
//! the composed `.cast`'s own embedded header theme (cloned verbatim from the
//! source recording by `compose`) is what "the recording's theme/font
//! settings" means here (AC#3), with no `asciicut-core` change needed.
//!
//! Command orchestration (native save dialog, `SessionState` locking) stays
//! in `commands.rs`'s `export_video`; this module is the pure/testable half —
//! path derivation, sidecar argument construction, `ffmpeg` progress-line
//! parsing, and [`run_export`], the post-dialog pipeline itself (spawn →
//! track → wait → emit), reusable directly by `tests/export_runtime_smoke.rs`
//! without driving a real save dialog.
//!
//! ## `ffmpeg`/`agg` quirks discovered while proving this pipeline
//!
//! (Not anticipated in the original plan — see PROGRESS.md for the writeback.)
//!
//! - `agg`'s GIF output width/height is whatever the terminal grid dictates
//!   and is frequently **odd** (e.g. 803px tall for the S1 sample recording).
//!   `libx264`/`libvpx-vp9` require even dimensions for 4:2:0 chroma
//!   subsampling and refuse to open the encoder otherwise ("Error while
//!   opening encoder... maybe incorrect parameters such as bit_rate, rate,
//!   width or height"). Both `mp4` and `webm` therefore run a
//!   `scale=trunc(iw/2)*2:trunc(ih/2)*2` filter first.
//! - `ffmpeg`'s default pixel format for a `libvpx-vp9` target fed from a
//!   GIF-with-alpha source is `gbrap`, which `libvpx-vp9` rejects outright
//!   ("Pixel format 'gbrap' is not widely supported"). `webm` needs an
//!   explicit `-pix_fmt yuv420p` (already required for `mp4` regardless).
//! - `-progress pipe:1`'s `out_time_ms=` key is a well-known `ffmpeg` misnomer:
//!   despite the name, its value is **microseconds** (numerically identical to
//!   the adjacent `out_time_us=` key), not milliseconds. [`percent_of`]
//!   divides by `1_000_000`, not `1_000` — confirmed against the bundled
//!   `ffmpeg` build by manual invocation during planning/implementation.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::ExportState;
use crate::sidecars::Sidecar;

/// The three export container/codec choices the SPA offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Mp4,
    Webm,
    Gif,
}

impl ExportFormat {
    /// Parse the SPA's lowercase format string.
    ///
    /// # Errors
    ///
    /// Returns an actionable message if `s` is not one of `"mp4"`, `"webm"`,
    /// or `"gif"`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "mp4" => Ok(ExportFormat::Mp4),
            "webm" => Ok(ExportFormat::Webm),
            "gif" => Ok(ExportFormat::Gif),
            other => Err(format!(
                "unknown export format '{other}' (expected mp4, webm, or gif)"
            )),
        }
    }

    /// File extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Mp4 => "mp4",
            ExportFormat::Webm => "webm",
            ExportFormat::Gif => "gif",
        }
    }

    /// Whether this format needs the `ffmpeg` transcode stage. `gif` stops at
    /// `agg`'s own output — `agg` has no other output format (SPEC §9).
    pub fn needs_ffmpeg(self) -> bool {
        !matches!(self, ExportFormat::Gif)
    }
}

/// The **desktop-local** composed-cast export target:
/// `<cast_dir>/<cast_stem>.composed.cast`. Shares
/// `asciicut-server::AppState::export_cast_path`'s derivation — the one in
/// `asciicut_bridge::sibling_path` — so the desktop and server exports cannot
/// drift apart on where a composed cast lands, even though `/api/export`
/// itself is deliberately outside the bridge's scope (CASTCU-S2-T02 plan).
pub fn export_cast_path(cast_path: &Path) -> PathBuf {
    asciicut_bridge::sibling_path(cast_path, asciicut_bridge::COMPOSED_CAST_SUFFIX)
}

/// The default save-dialog file name for an export: `<source_stem>.<ext>`.
pub fn default_output_name(source_name: &str, format: ExportFormat) -> String {
    let stem = source_name.strip_suffix(".cast").unwrap_or(source_name);
    format!("{stem}.{}", format.extension())
}

/// A process-unique intermediate GIF path for `mp4`/`webm` exports. The GIF
/// itself is not the deliverable — it is `ffmpeg`'s transcode source — and is
/// removed once the `ffmpeg` stage ends, success, failure, or cancellation.
pub fn temp_gif_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("asciicut-export-{}-{nanos}.gif", std::process::id()))
}

/// `agg` args to render the composed `.cast` into a GIF at `gif_target`. No
/// `--theme`/`--font-family` override — the composed cast's own embedded
/// header theme is what "the recording's theme/font settings" means (AC#3).
pub fn agg_args(cast_path: &Path, gif_target: &Path) -> Vec<String> {
    vec![
        cast_path.display().to_string(),
        gif_target.display().to_string(),
    ]
}

/// `ffmpeg` args to transcode an intermediate GIF into the final container.
/// `-y` overwrites the (dialog-confirmed) output path without prompting; the
/// `scale`/`pix_fmt` choices are the encoder-compatibility fixes documented in
/// this module's header comment. Panics if `format` is [`ExportFormat::Gif`]
/// (that format never reaches this function — [`ExportFormat::needs_ffmpeg`]
/// gates the call site).
pub fn ffmpeg_args(format: ExportFormat, gif_path: &Path, output_path: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        gif_path.display().to_string(),
        "-vf".into(),
        "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ];
    match format {
        ExportFormat::Mp4 => {
            args.push("-movflags".into());
            args.push("+faststart".into());
        }
        ExportFormat::Webm => {
            args.extend([
                "-c:v".into(),
                "libvpx-vp9".into(),
                "-b:v".into(),
                "0".into(),
                "-crf".into(),
                "34".into(),
                "-an".into(),
            ]);
        }
        ExportFormat::Gif => unreachable!("gif exports do not run ffmpeg"),
    }
    args.push("-progress".into());
    args.push("pipe:1".into());
    args.push(output_path.display().to_string());
    args
}

/// Parse one line of `ffmpeg -progress pipe:1` stdout, returning the elapsed
/// microseconds if the line is an `out_time_ms=` key (any other key —
/// `frame=`, `progress=`, `speed=`, etc. — yields `None`, including a
/// not-yet-started `out_time_ms=N/A`). See the module doc comment for why the
/// value is microseconds despite the key's name.
pub fn parse_out_time_us(line: &[u8]) -> Option<i64> {
    let line = std::str::from_utf8(line).ok()?.trim();
    let value = line.strip_prefix("out_time_ms=")?;
    value.trim().parse::<i64>().ok()
}

/// Parse one `agg` progress-bar segment into `(current_frame, total_frames)`.
///
/// `agg` streams its render progress to **stdout** (no `--verbose` needed) as a
/// carriage-return-redrawn bar shaped like:
///
/// ```text
///   83 / 10430 [>-------] 0.80 % 6.94/s 25m
/// ```
///
/// Only the leading `<current> / <total>` matters here — the bar, percent,
/// rate, and ETA that follow are re-derived by the caller. Returns `None` for
/// any segment that is not a progress line (a blank flush, a partial redraw, or
/// a `total` of zero, which would make a percentage meaningless).
pub fn parse_agg_progress(segment: &str) -> Option<(u64, u64)> {
    let (current, rest) = segment.trim().split_once('/')?;
    let current: u64 = current.trim().parse().ok()?;
    let total: u64 = rest.trim_start().split_whitespace().next()?.parse().ok()?;
    if total == 0 {
        return None;
    }
    Some((current, total))
}

/// Convert an elapsed-microseconds mark into a `0.0..=100.0` percent against
/// `cut_duration_secs`. Returns `None` for a non-finite/non-positive duration
/// (nothing sane to divide by) rather than emitting `NaN`/`inf`.
pub fn percent_of(out_time_us: i64, cut_duration_secs: f64) -> Option<f64> {
    if !(cut_duration_secs.is_finite() && cut_duration_secs > 0.0) {
        return None;
    }
    let total_us = cut_duration_secs * 1_000_000.0;
    Some((out_time_us as f64 / total_us * 100.0).clamp(0.0, 100.0))
}

/// Stage markers streamed to the SPA via the `export-progress` event.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportStage {
    WritingProject,
    WritingCast,
    Agg,
    Ffmpeg,
    Done,
    Error,
    Cancelled,
}

/// Payload for the `export-progress` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressPayload {
    pub stage: ExportStage,
    /// 0-100, only meaningful during the `ffmpeg` stage's fine-grained
    /// progress; the write stages and `agg` carry no percentage (an honest
    /// limitation — `agg` has no machine-readable progress output).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    /// Set on `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Set on `done` — the written video path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ExportProgressPayload {
    pub fn stage(stage: ExportStage) -> Self {
        Self {
            stage,
            percent: None,
            message: None,
            path: None,
        }
    }

    pub fn percent(stage: ExportStage, percent: f64) -> Self {
        Self {
            stage,
            percent: Some(percent),
            message: None,
            path: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            stage: ExportStage::Error,
            percent: None,
            message: Some(message.into()),
            path: None,
        }
    }

    pub fn done(path: impl Into<String>) -> Self {
        Self {
            stage: ExportStage::Done,
            percent: None,
            message: None,
            path: Some(path.into()),
        }
    }

    pub fn cancelled() -> Self {
        Self {
            stage: ExportStage::Cancelled,
            percent: None,
            message: None,
            path: None,
        }
    }
}

/// Best-effort `export-progress` emit — a dropped event is not fatal to the
/// export itself (the command's own awaited `Result` is the authoritative
/// terminal state; the event stream is a supplementary UX channel).
pub fn emit_progress<R: tauri::Runtime>(app: &AppHandle<R>, payload: ExportProgressPayload) {
    let _ = app.emit("export-progress", &payload);
}

/// Response for [`run_export`]/the `export_video` command: the paths written
/// (project, composed cast, video) plus the composed `.cast` document's byte
/// length — mirrors `asciicut-server::routes::ExportResponse`'s `bytes`
/// semantics (the composed document, not the video file).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub project_path: String,
    pub cast_path: String,
    pub video_path: String,
    pub bytes: usize,
}

/// Spawn `sidecar` with `args`, track its `CommandChild` in `export_state` so
/// a concurrent `cancel_export` can kill it, and wait for it to terminate.
/// `on_stdout` is called for every stdout chunk the child emits; stderr is
/// buffered so a failure can surface the sidecar's own message rather than a
/// bare exit code.
///
/// `raw_out` selects how stdout is framed. `ffmpeg`'s `-progress pipe:1` stream
/// is newline-delimited, so it uses the shell's default line reader
/// (`raw_out = false`) and `on_stdout` receives one line per call. `agg`'s
/// frame-render progress bar redraws with **carriage returns** and never emits
/// a newline until it finishes, so line-buffering would swallow it entirely —
/// it needs `raw_out = true`, delivering bytes exactly as received, and the
/// caller re-splits on `\r`/`\n` itself.
///
/// # Errors
///
/// - `"cancelled"` if `export_state`'s cancel flag was set during this run
///   (checked immediately after the child terminates, whether by exit or by
///   `cancel_export`'s kill).
/// - Otherwise a message naming the sidecar and, if any, its stderr tail.
pub async fn spawn_and_track<R: tauri::Runtime>(
    app: &AppHandle<R>,
    export_state: &ExportState,
    sidecar: Sidecar,
    args: Vec<String>,
    raw_out: bool,
    mut on_stdout: impl FnMut(&[u8]),
) -> Result<(), String> {
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;

    // Resolve next to the running app binary — the same directory
    // `Command::sidecar` itself resolves against (CASTCU-S2-T06; see
    // `sidecars.rs`'s module doc for why this is the exe dir, not
    // `app.path().resource_dir()`).
    let exe_dir = crate::sidecars::current_exe_dir().map_err(|e| e.to_string())?;
    crate::sidecars::resolve_checked(&exe_dir, sidecar).map_err(|e| e.to_string())?;

    let (mut rx, child) = app
        .shell()
        .sidecar(sidecar.name())
        .map_err(|e| format!("failed to prepare {} sidecar: {e}", sidecar.name()))?
        .args(args)
        .set_raw_out(raw_out)
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", sidecar.name()))?;

    export_state.track(child);

    let mut stderr_tail = String::new();
    let mut exit_status_ok = false;
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => on_stdout(&bytes),
            CommandEvent::Stderr(bytes) => {
                stderr_tail.push_str(&String::from_utf8_lossy(&bytes));
                stderr_tail.push('\n');
            }
            CommandEvent::Terminated(payload) => {
                exit_status_ok = payload.code == Some(0);
                break;
            }
            CommandEvent::Error(e) => {
                export_state.untrack();
                return Err(format!("{} error: {e}", sidecar.name()));
            }
            _ => {}
        }
    }
    export_state.untrack();

    if export_state.is_cancelled() {
        return Err("cancelled".to_string());
    }

    if exit_status_ok {
        Ok(())
    } else {
        let detail = stderr_tail.trim();
        Err(if detail.is_empty() {
            format!("{} exited with a non-zero status", sidecar.name())
        } else {
            format!("{} failed: {detail}", sidecar.name())
        })
    }
}

/// The post-dialog export pipeline: write the composed `.cast`, spawn `agg`
/// to render it to a GIF, then (for `mp4`/`webm`) spawn `ffmpeg` to transcode
/// that GIF into the final container, streaming `export-progress` events
/// throughout and cleaning up the intermediate GIF on every exit path.
///
/// Split out of the `export_video` `#[tauri::command]` so it is directly
/// testable against the real bundled sidecars without driving a native save
/// dialog (`tests/export_runtime_smoke.rs`) — `project_path`/`composed_text`/
/// `cast_export_path` are exactly what the command computes from the locked
/// `SessionState` before calling in.
///
/// # Errors
///
/// Returns `Err("cancelled")` if `export_state.cancel()` was called during the
/// run, or an actionable message otherwise (a write failure, a missing
/// sidecar bundle, or `agg`/`ffmpeg`'s own failure detail).
#[allow(clippy::too_many_arguments)]
pub async fn run_export<R: tauri::Runtime>(
    app: &AppHandle<R>,
    export_state: &ExportState,
    format: ExportFormat,
    cut_duration_secs: f64,
    video_path: PathBuf,
    project_path: String,
    composed_text: String,
    cast_export_path: PathBuf,
) -> Result<ExportResult, String> {
    export_state.begin();

    emit_progress(
        app,
        ExportProgressPayload::stage(ExportStage::WritingProject),
    );
    emit_progress(app, ExportProgressPayload::stage(ExportStage::WritingCast));
    if let Err(e) = std::fs::write(&cast_export_path, &composed_text) {
        let msg = format!("cannot write {}: {e}", cast_export_path.display());
        emit_progress(app, ExportProgressPayload::error(msg.clone()));
        return Err(msg);
    }

    if export_state.is_cancelled() {
        emit_progress(app, ExportProgressPayload::cancelled());
        return Err("cancelled".to_string());
    }

    emit_progress(app, ExportProgressPayload::stage(ExportStage::Agg));
    let gif_target = if format.needs_ffmpeg() {
        temp_gif_path()
    } else {
        video_path.clone()
    };

    // agg's frame-render is the pipeline's long pole (thousands of frames at a
    // few frames/sec) and, until now, the one stage with no progress feedback.
    // It actually streams a real per-frame bar to stdout — parse it into
    // `Agg` percent events. The bar redraws with `\r` and never a `\n`, so this
    // runs in raw-out mode and re-splits the byte chunks itself, keeping a
    // trailing partial redraw buffered until its terminator arrives. Emits are
    // throttled to whole-percent steps so a 10k-frame render sends ~100 events,
    // not 10k.
    let agg_app = app.clone();
    let mut agg_buf: Vec<u8> = Vec::new();
    let mut agg_last_pct: i64 = -1;
    let agg_result = spawn_and_track(
        app,
        export_state,
        Sidecar::Agg,
        agg_args(&cast_export_path, &gif_target),
        true,
        |chunk| {
            agg_buf.extend_from_slice(chunk);
            while let Some(pos) = agg_buf.iter().position(|&b| b == b'\r' || b == b'\n') {
                let segment: Vec<u8> = agg_buf.drain(..=pos).collect();
                let Ok(text) = std::str::from_utf8(&segment) else {
                    continue;
                };
                if let Some((current, total)) = parse_agg_progress(text) {
                    let pct = (current as f64 / total as f64) * 100.0;
                    if pct.floor() as i64 > agg_last_pct {
                        agg_last_pct = pct.floor() as i64;
                        emit_progress(
                            &agg_app,
                            ExportProgressPayload::percent(ExportStage::Agg, pct),
                        );
                    }
                }
            }
        },
    )
    .await;
    if let Err(e) = agg_result {
        let _ = std::fs::remove_file(&gif_target);
        return Err(terminal_error(app, e));
    }

    if format.needs_ffmpeg() {
        if export_state.is_cancelled() {
            let _ = std::fs::remove_file(&gif_target);
            emit_progress(app, ExportProgressPayload::cancelled());
            return Err("cancelled".to_string());
        }

        emit_progress(app, ExportProgressPayload::stage(ExportStage::Ffmpeg));
        let progress_app = app.clone();
        let ffmpeg_result = spawn_and_track(
            app,
            export_state,
            Sidecar::Ffmpeg,
            ffmpeg_args(format, &gif_target, &video_path),
            false,
            move |line| {
                if let Some(us) = parse_out_time_us(line) {
                    if let Some(pct) = percent_of(us, cut_duration_secs) {
                        emit_progress(
                            &progress_app,
                            ExportProgressPayload::percent(ExportStage::Ffmpeg, pct),
                        );
                    }
                }
            },
        )
        .await;
        let _ = std::fs::remove_file(&gif_target);

        if let Err(e) = ffmpeg_result {
            return Err(terminal_error(app, e));
        }
    }

    let result = ExportResult {
        project_path,
        cast_path: cast_export_path.display().to_string(),
        video_path: video_path.display().to_string(),
        bytes: composed_text.len(),
    };
    emit_progress(app, ExportProgressPayload::done(result.video_path.clone()));
    Ok(result)
}

/// Emit the terminal `cancelled`/`error` `export-progress` event for a failed
/// pipeline stage and return the same message, so the caller can `return
/// Err(terminal_error(...))` in one line.
fn terminal_error<R: tauri::Runtime>(app: &AppHandle<R>, message: String) -> String {
    if message == "cancelled" {
        emit_progress(app, ExportProgressPayload::cancelled());
    } else {
        emit_progress(app, ExportProgressPayload::error(message.clone()));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mp4_and_webm_and_gif() {
        assert_eq!(ExportFormat::parse("mp4").unwrap(), ExportFormat::Mp4);
        assert_eq!(ExportFormat::parse("webm").unwrap(), ExportFormat::Webm);
        assert_eq!(ExportFormat::parse("gif").unwrap(), ExportFormat::Gif);
        assert!(ExportFormat::parse("avi").is_err());
    }

    #[test]
    fn only_mp4_and_webm_need_ffmpeg() {
        assert!(ExportFormat::Mp4.needs_ffmpeg());
        assert!(ExportFormat::Webm.needs_ffmpeg());
        assert!(!ExportFormat::Gif.needs_ffmpeg());
    }

    #[test]
    fn export_cast_path_matches_server_derivation() {
        let cast_path = Path::new("/recordings/demo.cast");
        assert_eq!(
            export_cast_path(cast_path),
            PathBuf::from("/recordings/demo.composed.cast")
        );
    }

    #[test]
    fn default_output_name_swaps_extension() {
        assert_eq!(
            default_output_name("demo.cast", ExportFormat::Mp4),
            "demo.mp4"
        );
        assert_eq!(
            default_output_name("demo.cast", ExportFormat::Webm),
            "demo.webm"
        );
        assert_eq!(
            default_output_name("demo.cast", ExportFormat::Gif),
            "demo.gif"
        );
    }

    #[test]
    fn ffmpeg_args_include_even_dimension_scale_and_pix_fmt() {
        let gif = Path::new("/tmp/in.gif");
        let out = Path::new("/tmp/out.mp4");
        let args = ffmpeg_args(ExportFormat::Mp4, gif, out);
        assert!(args.contains(&"scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string()));
        assert!(args.contains(&"yuv420p".to_string()));
        assert!(args.contains(&"+faststart".to_string()));
    }

    #[test]
    fn webm_ffmpeg_args_force_yuv420p_for_libvpx_vp9() {
        let gif = Path::new("/tmp/in.gif");
        let out = Path::new("/tmp/out.webm");
        let args = ffmpeg_args(ExportFormat::Webm, gif, out);
        assert!(args.contains(&"libvpx-vp9".to_string()));
        assert!(args.contains(&"yuv420p".to_string()));
    }

    #[test]
    fn parse_out_time_us_reads_the_key_ffmpeg_calls_ms_but_emits_as_us() {
        assert_eq!(parse_out_time_us(b"out_time_ms=59702479"), Some(59_702_479));
        assert_eq!(parse_out_time_us(b"out_time_ms=N/A"), None);
        assert_eq!(parse_out_time_us(b"frame=41"), None);
        assert_eq!(parse_out_time_us(b"progress=end"), None);
    }

    #[test]
    fn parse_agg_progress_reads_current_and_total_frames() {
        // A representative redraw segment from the bundled agg 1.5.0 on stdout.
        assert_eq!(
            parse_agg_progress("  83 / 10430 [>-------] 0.80 % 6.94/s 25m"),
            Some((83, 10430))
        );
        // First frame and the final 100% frame both parse.
        assert_eq!(parse_agg_progress("1 / 10430 [>] 0.01 % 1.28/s 136m"), Some((1, 10430)));
        assert_eq!(
            parse_agg_progress("10430 / 10430 [=====] 100.00 % 7.0/s 0m"),
            Some((10430, 10430))
        );
    }

    #[test]
    fn parse_agg_progress_rejects_non_progress_and_zero_total() {
        assert_eq!(parse_agg_progress(""), None);
        assert_eq!(parse_agg_progress("   "), None);
        assert_eq!(parse_agg_progress("rendering"), None);
        // A zero total would make the percentage a divide-by-zero.
        assert_eq!(parse_agg_progress("0 / 0 [] 0 %"), None);
    }

    #[test]
    fn percent_of_divides_by_one_million_not_one_thousand() {
        // 59_702_479 us over a 60s clip ≈ 99.5%, not the wildly-wrong figure a
        // ms-not-us assumption would produce (>100% instantly).
        let pct = percent_of(59_702_479, 60.0).unwrap();
        assert!((pct - 99.5).abs() < 0.5, "got {pct}");
    }

    #[test]
    fn percent_of_clamps_and_rejects_bad_duration() {
        assert_eq!(percent_of(999_999_999, 1.0), Some(100.0));
        assert_eq!(percent_of(0, 0.0), None);
        assert_eq!(percent_of(0, f64::NAN), None);
        assert_eq!(percent_of(0, -1.0), None);
    }
}
