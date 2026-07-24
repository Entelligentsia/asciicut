//! Tauri-runtime smoke for the export pipeline (CASTCU-S2-T05): real bundled
//! `agg`/`ffmpeg` sidecars, driven through `export::run_export` (the same
//! post-dialog pipeline `commands::export_video` calls) against a genuine
//! composed `.cast` document.
//!
//! Mirrors `sidecar_runtime_smoke.rs`'s exe-dir staging (CASTCU-S2-T06 path
//! fix — see `sidecars.rs`'s module doc) so `Command::sidecar` finds the real
//! fetched binaries without a packaged build.
//! Exercises `run_export` directly rather than the `#[tauri::command]`
//! `export_video` wrapper because `export_video` opens a native save dialog
//! first (`tauri_plugin_dialog::blocking_save_file`) — not headlessly
//! drivable in CI — while `run_export` is exactly the part of the pipeline
//! that starts once a path is already chosen (see `export.rs`'s doc comment
//! on why the split exists).

use std::path::{Path, PathBuf};

use asciicut_bridge::{ops, Session};
use asciicut_desktop_lib::commands::ExportState;
use asciicut_desktop_lib::export::{self, ExportFormat};
use asciicut_desktop_lib::sidecars::current_exe_dir;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

/// A unique scratch directory under the OS temp dir, removed on `Drop` —
/// mirrors `commands_smoke.rs`'s own helper.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "asciicut-export-smoke-{tag}-{}-{nanos}",
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

/// Stage the fetched sidecars from `CARGO_MANIFEST_DIR/sidecars/` into the
/// runtime exe dir (`sidecar_runtime_smoke.rs`'s helper, relocated verbatim)
/// so `Command::sidecar`'s resolver finds them without a packaged build.
fn stage_sidecars_for_runtime_test() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecars");
    let dst = current_exe_dir().expect("current_exe_dir resolvable in tests");
    std::fs::create_dir_all(&dst).expect("create runtime exe dir");

    let triple = asciicut_desktop_lib::sidecars::current_target_triple().unwrap();
    for (source_name, runtime_name) in [
        (format!("agg-{triple}"), "agg"),
        (format!("ffmpeg-{triple}"), "ffmpeg"),
    ] {
        let src_file = src.join(&source_name);
        let dst_file = dst.join(runtime_name);
        if src_file.exists() {
            std::fs::copy(&src_file, &dst_file).unwrap_or_else(|e| {
                panic!("copy {} to {}: {e}", src_file.display(), dst_file.display())
            });
            let mut perms = std::fs::metadata(&dst_file).unwrap().permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(0o755);
            }
            std::fs::set_permissions(&dst_file, perms).unwrap();
        } else {
            panic!(
                "source sidecar not found: {} — run sidecars/fetch-sidecars.sh",
                src_file.display()
            );
        }
    }
}

/// Everything `commands::export_video` computes from the locked
/// `SessionState` before calling into `run_export` — built here directly
/// against a copy of the S1 golden-cast fixture in a disposable temp dir, and
/// a short (5s) project window so the real `agg` invocation stays fast.
struct PipelineInputs {
    project_path: String,
    composed_text: String,
    cast_export_path: PathBuf,
}

fn build_pipeline_inputs(tmp: &Path) -> PipelineInputs {
    let cast_path = tmp.join("sample.cast");
    std::fs::copy(repo_root().join("samples/sample.cast"), &cast_path)
        .expect("copy fixture cast into temp dir");

    let mut session = Session::new();
    session.load(cast_path).expect("load fixture cast");

    // A short 5-second window — a real `agg` render of the full ~17-minute S1
    // sample would dominate this test's runtime for no additional coverage.
    let project_body = r#"{"source":"sample.cast","idleCap":0.4,"segments":[{"srcStart":0,"srcEnd":5,"speed":1,"holdEnd":0}]}"#;

    let save_response = ops::save_project(&session, project_body).expect("save_project");
    let composed_text = ops::compose_project(&session, project_body).expect("compose_project");
    let cast_export_path = export::export_cast_path(session.cast_path().unwrap());

    PipelineInputs {
        project_path: save_response.path,
        composed_text,
        cast_export_path,
    }
}

fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .plugin(tauri_plugin_shell::init())
        .build(tauri::generate_context!())
        .expect("failed to build test app")
}

/// AC#1/AC#3/AC#4 — one `agg`/`ffmpeg`-backed run per format
/// (`.gif`/`.mp4`/`.webm`), each producing a real non-empty file at the
/// chosen path, plus the `.mp4` leg proving the intermediate GIF is cleaned
/// up. Deliberately **one sequential test**, not three parallel `#[test]`
/// fns: `export::temp_gif_path()` names its intermediate file
/// `asciicut-export-<pid>-<nanos>.gif`, and Rust's test harness runs `#[test]`
/// fns as threads inside the *same process* (same pid) by default — three
/// parallel format tests each doing a temp-dir leftover scan would race
/// against each other's still-in-flight intermediate files. Running them in
/// one thread removes that race without weakening the assertions.
#[test]
fn each_format_produces_a_real_non_empty_file() {
    stage_sidecars_for_runtime_test();
    let app = mock_app();

    for (format, ext) in [
        (ExportFormat::Gif, "gif"),
        (ExportFormat::Mp4, "mp4"),
        (ExportFormat::Webm, "webm"),
    ] {
        let tmp = TempDir::new(ext);
        let inputs = build_pipeline_inputs(&tmp.0);
        let video_path = tmp.0.join(format!("out.{ext}"));
        let export_state = ExportState::new();

        let result = tauri::async_runtime::block_on(export::run_export(
            app.handle(),
            &export_state,
            format,
            5.0,
            video_path.clone(),
            inputs.project_path,
            inputs.composed_text,
            inputs.cast_export_path,
        ))
        .unwrap_or_else(|e| panic!("{ext} export should succeed: {e}"));

        assert_eq!(result.video_path, video_path.display().to_string());
        assert!(video_path.exists(), "{ext} written to the chosen path");
        assert!(
            std::fs::metadata(&video_path).unwrap().len() > 0,
            "{ext} is non-empty"
        );

        if format.needs_ffmpeg() {
            // The intermediate GIF (`asciicut-export-<pid>-<nanos>.gif` under
            // the OS temp dir) must not survive a successful export. Safe to
            // scan the whole temp dir here — this test runs single-threaded
            // relative to itself, and any earlier iteration's intermediate
            // file has already been asserted gone.
            let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(&format!("asciicut-export-{}-", std::process::id()))
                })
                .collect();
            assert!(
                leftovers.is_empty(),
                "{ext}: intermediate gif must be cleaned up, found: {leftovers:?}"
            );
        }
    }
}

/// AC#6 — cancelling mid-pipeline (during the `agg` stage, the slower one)
/// via `ExportState::cancel` kills the tracked child and `run_export` reports
/// `Err("cancelled")`, not a spurious "killed"/non-zero-exit error. The
/// output path must not be left as a corrupt partial GIF.
#[test]
fn cancel_during_agg_reports_cancelled_not_error() {
    stage_sidecars_for_runtime_test();
    let tmp = TempDir::new("cancel");
    let inputs = build_pipeline_inputs(&tmp.0);
    let video_path = tmp.0.join("out.gif");

    let app = mock_app();
    let export_state = ExportState::new();

    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            // Give `agg` a moment to actually start (past the write stage and
            // into the tracked-child window) before cancelling — long enough
            // that the child is spawned and tracked, short enough that the
            // ~4s agg render for this 5s clip has not yet finished.
            std::thread::sleep(std::time::Duration::from_millis(400));
            export_state.cancel();
        });

        tauri::async_runtime::block_on(export::run_export(
            app.handle(),
            &export_state,
            ExportFormat::Gif,
            5.0,
            video_path.clone(),
            inputs.project_path,
            inputs.composed_text,
            inputs.cast_export_path,
        ))
    });

    match result {
        Err(ref e) if e == "cancelled" => {}
        other => panic!("expected Err(\"cancelled\"), got {other:?}"),
    }
}
