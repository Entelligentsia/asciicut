//! Detached sidecar resolver tests (CASTCU-S2-T04; path fix CASTCU-S2-T06).
//!
//! These tests exercise the triple mapping, path resolution, and missing-bundle
//! error path without a Tauri runtime. They prove the resolver is usable from
//! both unit tests and the desktop shell, and that the resolver checks the
//! **exe dir** — where `tauri-build` (dev) and `tauri-bundler` (packaged
//! installs) actually place `externalBin` sidecars, confirmed against a real
//! built `.deb`/AppImage in CASTCU-S2-T06 — not the old `resource_dir/sidecars/`
//! path Tauri never populates. See `sidecars.rs`'s module doc for the full story.

use std::path::PathBuf;

use asciicut_desktop_lib::sidecars::{self, Sidecar, SidecarError};

/// The current platform has a deterministic target triple.
#[test]
fn current_triple_is_supported_on_this_host() {
    let triple =
        sidecars::current_target_triple().expect("this CI/host platform must be supported");
    assert!(!triple.is_empty());
    assert!(triple.contains('-'));
}

/// Supported triples follow the Tauri `externalBin` naming convention.
#[test]
fn file_name_uses_tauri_external_bin_naming() {
    assert_eq!(
        sidecars::sidecar_file_name("agg", "x86_64-unknown-linux-gnu"),
        "agg-x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        sidecars::sidecar_file_name("ffmpeg", "aarch64-apple-darwin"),
        "ffmpeg-aarch64-apple-darwin"
    );
}

/// The resolver places files directly under the caller-supplied `exe_dir`,
/// bare-named (no target-triple suffix, no `sidecars/` subfolder) — exactly
/// where a packaged install's `usr/bin/agg` (or dev-mode `target/debug/agg`)
/// actually lands. The `-<triple>` suffix only exists on the *source* file
/// `fetch-sidecars.sh` writes; Tauri strips it before this directory is ever
/// consulted.
#[test]
fn resolve_uses_exe_dir_bare_name_no_triple() {
    let exe_dir = PathBuf::from("/tmp/fake-exe-dir");
    let path = sidecars::resolve(&exe_dir, Sidecar::Agg).expect("supported triple");
    let expected_name = if cfg!(windows) { "agg.exe" } else { "agg" };
    assert_eq!(path, exe_dir.join(expected_name));
}

/// A missing bundle must produce a `MissingSidecar` error that names the sidecar
/// and its expected path, not a PATH fallback or a generic IO error.
#[test]
fn missing_bundle_returns_actionable_error() {
    let tmp = std::env::temp_dir().join(format!(
        "asciicut-sidecar-test-missing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let err =
        sidecars::resolve_checked(&tmp, Sidecar::Ffmpeg).expect_err("missing ffmpeg must error");
    assert!(
        matches!(err, SidecarError::MissingSidecar { name: "ffmpeg", .. }),
        "expected MissingSidecar for ffmpeg, got: {err}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("fetch-sidecars.sh"),
        "error message should point at fetch script: {msg}"
    );
    assert!(
        msg.contains("README.md"),
        "error message should point at README: {msg}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// When the expected file exists, `resolve_checked` returns its absolute path.
#[test]
fn present_bundle_resolves_successfully() {
    let tmp = std::env::temp_dir().join(format!(
        "asciicut-sidecar-test-present-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let expected = sidecars::resolve(&tmp, Sidecar::Ffmpeg).unwrap();
    std::fs::write(&expected, b"fake ffmpeg binary").unwrap();

    let actual =
        sidecars::resolve_checked(&tmp, Sidecar::Ffmpeg).expect("present bundle should resolve");
    assert_eq!(actual, expected);

    let _ = std::fs::remove_dir_all(&tmp);
}

/// The two sidecars share the same exe-dir layout but produce different
/// (bare, triple-free) file names.
#[test]
fn agg_and_ffmpeg_resolve_to_distinct_paths() {
    let tmp = PathBuf::from("/tmp/fake-exe-dir");
    let agg = sidecars::resolve(&tmp, Sidecar::Agg).unwrap();
    let ffmpeg = sidecars::resolve(&tmp, Sidecar::Ffmpeg).unwrap();
    assert_ne!(agg, ffmpeg);
    assert!(agg
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("agg"));
    assert!(ffmpeg
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("ffmpeg"));
}
