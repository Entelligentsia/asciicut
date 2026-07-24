//! Tauri-runtime smoke for the bundled `agg` and `ffmpeg` sidecars (CASTCU-S2-T04;
//! path fix CASTCU-S2-T06).
//!
//! `tauri_plugin_shell::Command::sidecar` (and, since T06, this crate's own
//! `sidecars::resolve_checked` preflight) resolve the bundled binary relative
//! to `std::env::current_exe()`'s directory (one level up out of `deps/` for
//! a `cargo test` binary) — see `sidecars.rs`'s module doc for why this is
//! the exe dir, not `app.path().resource_dir()`. This test pre-stages the
//! fetched sidecars there, bare-named (no target-triple suffix, matching
//! what `tauri-build`'s dev-mode staging / `tauri-bundler`'s packaging both
//! produce), so the real `agg_version` / `ffmpeg_version` commands can
//! exercise the full `invoke()` → Rust command → `tauri-plugin-shell` →
//! bundled sidecar path.

use std::path::PathBuf;

use asciicut_desktop_lib::commands::{agg_version, ffmpeg_version};
use asciicut_desktop_lib::sidecars::current_exe_dir;

/// Stage the fetched sidecars from `CARGO_MANIFEST_DIR/sidecars/` into the
/// runtime exe dir so `Command::sidecar`'s resolver finds them without a
/// packaged build.
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
            // Make the copied sidecar executable. Unix-only: on Windows the
            // `.exe` extension makes it runnable and there is no +x bit, so the
            // whole block (and its `mut perms`) would be dead there.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&dst_file).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&dst_file, perms).unwrap();
            }
        } else {
            panic!(
                "source sidecar not found: {} — run sidecars/fetch-sidecars.sh",
                src_file.display()
            );
        }
    }
}

#[test]
fn agg_version_runs_bundled_sidecar() {
    stage_sidecars_for_runtime_test();

    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_shell::init())
        .build(tauri::generate_context!())
        .expect("failed to build test app");

    let version = tauri::async_runtime::block_on(async move {
        agg_version(app.handle().clone())
            .await
            .expect("agg --version should succeed")
    });

    assert!(
        version.contains("agg") || version.contains("asciinema"),
        "agg --version should identify itself, got: {version}"
    );
}

#[test]
fn ffmpeg_version_runs_bundled_sidecar() {
    stage_sidecars_for_runtime_test();

    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_shell::init())
        .build(tauri::generate_context!())
        .expect("failed to build test app");

    let version = tauri::async_runtime::block_on(async move {
        ffmpeg_version(app.handle().clone())
            .await
            .expect("ffmpeg -version should succeed")
    });

    assert!(
        version.contains("ffmpeg") && version.contains("version"),
        "ffmpeg -version should identify itself, got: {version}"
    );
}
