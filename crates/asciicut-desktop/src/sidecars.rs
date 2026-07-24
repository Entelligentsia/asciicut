//! Sidecar binary discovery and invocation helpers (CASTCU-S2-T04, path fix
//! CASTCU-S2-T06).
//!
//! `agg` and `ffmpeg` are declared as `bundle.externalBin` sidecars in
//! `tauri.conf.json`. The **source** files this repo stages via
//! `sidecars/fetch-sidecars.sh` are named `<name>-<target-triple>` (e.g.
//! `sidecars/agg-x86_64-unknown-linux-gnu`) — that per-triple naming is what
//! lets `tauri-build`'s dev-mode staging and `tauri-bundler`'s packaging step
//! pick the right binary for the host/target. But **neither** dev mode nor a
//! packaged install exposes that file under `app.path().resource_dir()`:
//!
//! - `tauri-build`'s `build.rs` step copies the matching source file next to
//!   the compiled binary for local `cargo run`/`cargo test` use — e.g.
//!   `target/debug/agg` — stripped of the target-triple suffix.
//! - `tauri-bundler` does the exact same stripping for a real installer: the
//!   Linux `.deb`/AppImage produced by this task place the sidecars at
//!   `usr/bin/agg` / `usr/bin/ffmpeg`, next to `usr/bin/asciicut-desktop` —
//!   confirmed by inspecting the real bundle output in this task, not a
//!   resource-dir subfolder.
//! - `tauri_plugin_shell`'s `Command::sidecar(name)` (what `commands.rs` /
//!   `export.rs` actually spawn through) resolves the binary the same way:
//!   relative to `std::env::current_exe()`'s directory, by the bare name with
//!   no target-triple suffix (see `tauri-plugin-shell`'s private
//!   `relative_command_path`). It never consults `resource_dir()`.
//!
//! An earlier version of this module resolved against
//! `resource_dir/sidecars/<name>-<triple>` — a path Tauri never populates in
//! either dev or packaged mode, discovered when CASTCU-S2-T06's packaged-build
//! smoke test ran `resolve_checked` against the real installed `.deb` and it
//! reported the sidecar missing even though `usr/bin/agg` was right there.
//! This module now resolves next to the **running executable** — the same
//! directory `Command::sidecar` itself uses — so the preflight check matches
//! reality in dev, test, and packaged installs alike.
//!
//! The resolver is intentionally independent of a live Tauri app: it takes a
//! caller-supplied `exe_dir` (`std::env::current_exe()`'s parent in
//! production, a temp dir in unit tests). That keeps the missing-bundle logic
//! testable without spawning a webview or relying on `tauri::test`.

use std::path::{Path, PathBuf};

/// Errors that can occur when resolving a bundled sidecar.
#[derive(Debug, Clone, PartialEq)]
pub enum SidecarError {
    /// The current OS/arch combination is not in the supported matrix.
    UnsupportedPlatform {
        os: &'static str,
        arch: &'static str,
    },
    /// The expected bundled file does not exist next to the app binary.
    MissingSidecar {
        name: &'static str,
        expected_path: PathBuf,
    },
    /// The running executable's own directory could not be determined, so
    /// there is no directory to resolve a sidecar against.
    ExeDirUnavailable(String),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::UnsupportedPlatform { os, arch } => {
                write!(f, "unsupported platform: {os}/{arch}")
            }
            SidecarError::MissingSidecar { name, expected_path } => write!(
                f,
                "sidecar '{name}' not found next to the app binary at '{}'; run sidecars/fetch-sidecars.sh for this platform and rebuild/repackage (see sidecars/README.md and BUILD.md)",
                expected_path.display()
            ),
            SidecarError::ExeDirUnavailable(detail) => {
                write!(f, "cannot locate the running app binary: {detail}")
            }
        }
    }
}

impl std::error::Error for SidecarError {}

/// Sidecars bundled with the desktop app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sidecar {
    /// The asciinema GIF encoder (GPL v3) — `agg`.
    Agg,
    /// The ffmpeg static build used for mp4/webm/gif export.
    Ffmpeg,
}

impl Sidecar {
    /// Base binary name, matching the `externalBin` declaration in
    /// `tauri.conf.json` (without target-triple suffix).
    pub fn name(self) -> &'static str {
        match self {
            Sidecar::Agg => "agg",
            Sidecar::Ffmpeg => "ffmpeg",
        }
    }
}

/// Return the Rust target triple Tauri uses for the current OS/arch.
///
/// Kept as a standalone function so tests can assert the mapping table without
/// a live sidecar present.
pub fn current_target_triple() -> Result<&'static str, SidecarError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => Err(SidecarError::UnsupportedPlatform { os, arch }),
    }
}

/// Build the **source** sidecar file name for a target triple — the naming
/// convention `sidecars/fetch-sidecars.sh` writes and `tauri.conf.json`'s
/// `bundle.externalBin` reads at build/bundle time. Not the runtime path (see
/// [`resolve`]): both `tauri-build` (dev) and `tauri-bundler` (packaged)
/// strip this suffix before placing the file next to the app binary.
pub fn sidecar_file_name(name: &str, triple: &str) -> String {
    format!("{name}-{triple}")
}

/// Platform-appropriate bare binary file name Tauri places next to the app
/// executable at runtime (dev-mode `build.rs` staging and packaged-install
/// bundling both strip the target-triple suffix and, on Windows, add `.exe`).
fn runtime_file_name(sidecar: Sidecar) -> String {
    if cfg!(windows) {
        format!("{}.exe", sidecar.name())
    } else {
        sidecar.name().to_string()
    }
}

/// Resolve the expected sidecar path inside `exe_dir` — the directory
/// containing the running app binary, matching exactly where
/// `tauri_plugin_shell::Command::sidecar` resolves its own target (dev-mode
/// `target/debug|release/<name>`, or a packaged install's `usr/bin/<name>` /
/// `Contents/MacOS/<name>` / `<name>.exe` next to the main executable). This
/// does **not** verify existence; use [`resolve_checked`] to get an
/// actionable error when the bundle is absent. Still validates that the
/// current OS/arch is in the supported matrix (an unsupported platform has no
/// sidecar to find, regardless of directory).
pub fn resolve(exe_dir: &Path, sidecar: Sidecar) -> Result<PathBuf, SidecarError> {
    current_target_triple()?;
    Ok(exe_dir.join(runtime_file_name(sidecar)))
}

/// Resolve the sidecar path and verify the file exists.
///
/// This is the production entry point used before spawning a sidecar process.
/// It rejects a missing bundle with a clear message pointing at the fetch
/// script and README rather than falling back to `$PATH` (D7).
pub fn resolve_checked(exe_dir: &Path, sidecar: Sidecar) -> Result<PathBuf, SidecarError> {
    let path = resolve(exe_dir, sidecar)?;
    if path.exists() {
        Ok(path)
    } else {
        Err(SidecarError::MissingSidecar {
            name: sidecar.name(),
            expected_path: path,
        })
    }
}

/// The directory containing the currently running executable — the directory
/// [`resolve`]/[`resolve_checked`] must be called against in production so
/// the preflight check matches where `Command::sidecar` actually looks.
pub fn current_exe_dir() -> Result<PathBuf, SidecarError> {
    // Both failures here are "we cannot find ourselves on disk", NOT "this
    // platform is unsupported" — reporting them as the latter would send the
    // reader to the sidecar fetch script for a problem it cannot fix.
    let exe = std::env::current_exe()
        .map_err(|e| SidecarError::ExeDirUnavailable(format!("current_exe failed: {e}")))?;
    let dir = exe.parent().ok_or_else(|| {
        SidecarError::ExeDirUnavailable(format!("'{}' has no parent directory", exe.display()))
    })?;
    // `cargo test` binaries live under `target/{debug,release}/deps/`; both
    // `tauri-build`'s dev-mode staging and `tauri_plugin_shell`'s own
    // `relative_command_path` walk up one level in that case so sidecar
    // lookups land in `target/{debug,release}/`, matching where the fetched
    // binaries are actually staged for local runs.
    let base = if dir.ends_with("deps") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    Ok(base.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_names_match_external_bin_declaration() {
        assert_eq!(Sidecar::Agg.name(), "agg");
        assert_eq!(Sidecar::Ffmpeg.name(), "ffmpeg");
    }

    #[test]
    fn file_name_includes_target_triple() {
        assert_eq!(
            sidecar_file_name("agg", "x86_64-unknown-linux-gnu"),
            "agg-x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn resolve_maps_next_to_exe_dir_no_triple_no_subdir() {
        let tmp = std::env::temp_dir();
        let path = resolve(&tmp, Sidecar::Agg).expect("linux dev host is supported");
        // Matches tauri-bundler/build.rs output exactly: bare name, directly
        // inside the exe's own directory — no "sidecars" subfolder, no
        // target-triple suffix (that suffix only exists on the *source* file
        // `fetch-sidecars.sh` writes, per `sidecar_file_name`).
        assert_eq!(path.parent().unwrap(), tmp);
        let expected_name = if cfg!(windows) { "agg.exe" } else { "agg" };
        assert_eq!(path.file_name().unwrap(), expected_name);
    }

    #[test]
    fn current_exe_dir_strips_trailing_deps_segment() {
        // `cargo test` binaries run from `target/{debug,release}/deps/`;
        // `current_exe_dir` must land one level up, matching where
        // `tauri-build`'s dev-mode staging puts `agg`/`ffmpeg`.
        let dir = current_exe_dir().expect("current_exe resolvable in tests");
        assert!(
            !dir.ends_with("deps"),
            "current_exe_dir must not end in deps: {}",
            dir.display()
        );
    }

    #[test]
    fn resolve_checked_errors_on_missing_bundle() {
        let tmp = std::env::temp_dir().join(format!(
            "asciicut-sidecar-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let err = resolve_checked(&tmp, Sidecar::Agg).expect_err("missing bundle must error");
        assert!(
            matches!(err, SidecarError::MissingSidecar { name: "agg", .. }),
            "error must name the sidecar and its expected path: {err}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_checked_succeeds_when_bundle_present() {
        let tmp = std::env::temp_dir().join(format!(
            "asciicut-sidecar-present-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let expected = resolve(&tmp, Sidecar::Agg).unwrap();
        std::fs::write(&expected, b"#!/bin/sh\necho fake agg").unwrap();

        let actual = resolve_checked(&tmp, Sidecar::Agg).expect("present bundle resolves");
        assert_eq!(actual, expected);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
