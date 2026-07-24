//! Packaged-build smoke (CASTCU-S2-T06): proves the **real, `tauri build`-produced**
//! Linux installers — not the dev/debug binary earlier tasks' smoke tests use —
//! actually work: the resolver in `sidecars.rs` finds the bundled `agg`/`ffmpeg`
//! at the exact path the installer places them, those exact binaries run and
//! identify themselves, and they can perform a real compose → agg → ffmpeg
//! export against a genuine `.cast` fixture.
//!
//! Two Linux artifact layouts are checked, both produced by
//! `crates/asciicut-desktop/scripts/smoke-packaged-build.sh`'s `tauri build
//! --bundles deb,appimage` step:
//!
//! - the `.deb`, extracted with `dpkg-deb -x` (a real installed-tree layout —
//!   `usr/bin/{asciicut-desktop,agg,ffmpeg}`);
//! - the AppImage's `asciicut.AppDir` — `tauri-bundler` writes this as a plain
//!   unpacked directory *before* squashing it into the `.AppImage`, so it is
//!   inspected directly with no FUSE mount needed (PLAN_REVIEW advisory #3).
//!
//! This does **not** drive the packaged binary's own webview/IPC (that needs
//! `tauri-driver`/WebDriver, out of reach here — PLAN_REVIEW advisory #1); the
//! native-window/launch/`.cast`-argument half of AC#2 is proven separately by
//! `scripts/smoke-packaged-build.sh`'s Xvfb + `xwininfo` step (mirroring
//! CASTCU-S2-T01's own pattern). What this file proves is the other half:
//! that the production `sidecars::resolve_checked` resolver — exercised here
//! against the *real* packaged layout, not a synthetic temp dir — finds the
//! exact bundled binaries, and that those exact binaries perform a real
//! compose+export.
//!
//! `#[ignore]`d by default: it depends on `target/release/bundle/` existing,
//! which needs a prior `tauri build --bundles deb,appimage` (a multi-minute,
//! multi-hundred-MB step) — not something a plain `cargo test` in a fresh
//! checkout should require. Run explicitly via
//! `cargo test --manifest-path crates/asciicut-desktop/Cargo.toml --test packaged_build_smoke -- --ignored --nocapture`
//! (exactly what `scripts/smoke-packaged-build.sh` does) after building the
//! bundles.

use std::path::{Path, PathBuf};
use std::process::Command;

use asciicut_bridge::{ops, Session};
use asciicut_desktop_lib::export::{self, ExportFormat};
use asciicut_desktop_lib::sidecars::{self, Sidecar};

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

fn bundle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/bundle")
}

/// A unique scratch directory under the OS temp dir, removed on `Drop`.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "asciicut-packaged-smoke-{tag}-{}-{nanos}",
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

/// Find the single `.deb` under `target/release/bundle/deb/`, extract it with
/// the real `dpkg-deb -x`, and return the extracted `usr/bin/` directory —
/// the directory containing the real, installer-placed
/// `asciicut-desktop`/`agg`/`ffmpeg`.
fn extract_deb_usr_bin(scratch: &Path) -> PathBuf {
    let deb_dir = bundle_dir().join("deb");
    let deb_path = std::fs::read_dir(&deb_dir)
        .unwrap_or_else(|e| {
            panic!(
                "read {}: {e} — run scripts/smoke-packaged-build.sh's build step first",
                deb_dir.display()
            )
        })
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|ext| ext == "deb"))
        .unwrap_or_else(|| panic!("no .deb found under {}", deb_dir.display()));

    let dest = scratch.join("deb-extracted");
    std::fs::create_dir_all(&dest).unwrap();
    let status = Command::new("dpkg-deb")
        .args(["-x", deb_path.to_str().unwrap(), dest.to_str().unwrap()])
        .status()
        .expect("dpkg-deb must be installed to run this smoke");
    assert!(
        status.success(),
        "dpkg-deb -x failed on {}",
        deb_path.display()
    );

    dest.join("usr/bin")
}

/// The AppImage's already-unpacked `asciicut.AppDir/usr/bin` — no FUSE mount
/// needed, since `tauri-bundler` builds this plain directory before
/// squashing it into the final `.AppImage`.
fn appimage_appdir_usr_bin() -> PathBuf {
    let dir = bundle_dir().join("appimage/asciicut.AppDir/usr/bin");
    assert!(
        dir.is_dir(),
        "{} not found — run scripts/smoke-packaged-build.sh's build step first",
        dir.display()
    );
    dir
}

/// For a given real installed-layout `usr/bin` directory: the production
/// resolver finds `agg`/`ffmpeg` exactly there (AC#4), and the resolved
/// binaries actually run and identify themselves.
fn assert_layout_resolves_and_runs(usr_bin: &Path, label: &str) {
    for (sidecar, version_flag, expect_prefix) in [
        (Sidecar::Agg, "--version", "agg"),
        (Sidecar::Ffmpeg, "-version", "ffmpeg"),
    ] {
        let resolved = sidecars::resolve_checked(usr_bin, sidecar).unwrap_or_else(|e| {
            panic!(
                "{label}: resolve_checked({}) failed against the real packaged layout: {e}",
                sidecar.name()
            )
        });
        assert_eq!(
            resolved,
            usr_bin.join(sidecar.name()),
            "{label}: resolver must point at the bundled binary itself"
        );

        let output = Command::new(&resolved)
            .arg(version_flag)
            .output()
            .unwrap_or_else(|e| panic!("{label}: spawn {}: {e}", resolved.display()));
        assert!(
            output.status.success(),
            "{label}: {} {version_flag} exited non-zero",
            resolved.display()
        );
        let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
        assert!(
            text.contains(expect_prefix),
            "{label}: {} output should mention '{expect_prefix}', got: {text}",
            sidecar.name()
        );
    }
}

/// AC#4 — the production resolver, run against both real packaged Linux
/// layouts (not a synthetic temp dir), finds the bundled sidecars exactly
/// where the installer placed them, and those exact binaries run.
#[test]
#[ignore = "requires a prior `tauri build --bundles deb,appimage` — see module doc"]
fn resolver_finds_and_runs_real_bundled_sidecars_in_both_layouts() {
    let scratch = TempDir::new("resolve");
    let deb_usr_bin = extract_deb_usr_bin(&scratch.0);
    assert_layout_resolves_and_runs(&deb_usr_bin, "deb");

    let appimage_usr_bin = appimage_appdir_usr_bin();
    assert_layout_resolves_and_runs(&appimage_usr_bin, "appimage");
}

/// AC#2/AC#4 — a real compose → `agg` → `ffmpeg` export, using the exact
/// binaries the `.deb` installer bundles (found via the production resolver
/// against the real installed-tree layout), against a genuine `.cast`
/// fixture. Mirrors `export_runtime_smoke.rs`'s pipeline construction but
/// spawns the packaged binaries directly with `std::process::Command`
/// (equivalent to what `tauri-plugin-shell`'s `Command::sidecar` does
/// underneath, minus the Tauri IPC/ACL layer that needs a live app — the
/// part of this AC genuinely out of reach without `tauri-driver`).
#[test]
#[ignore = "requires a prior `tauri build --bundles deb,appimage` — see module doc"]
fn real_bundled_sidecars_perform_a_real_export() {
    let scratch = TempDir::new("export");
    let usr_bin = extract_deb_usr_bin(&scratch.0);
    let agg_path = sidecars::resolve_checked(&usr_bin, Sidecar::Agg)
        .expect("agg resolves against the real deb layout");
    let ffmpeg_path = sidecars::resolve_checked(&usr_bin, Sidecar::Ffmpeg)
        .expect("ffmpeg resolves against the real deb layout");

    let work = TempDir::new("export-work");
    let cast_path = work.0.join("sample.cast");
    std::fs::copy(repo_root().join("samples/sample.cast"), &cast_path)
        .expect("copy fixture cast into temp dir");

    let mut session = Session::new();
    session.load(cast_path).expect("load fixture cast");

    // Short 5s window, mirroring export_runtime_smoke.rs — keeps the real
    // `agg` render fast without weakening the proof.
    let project_body = r#"{"source":"sample.cast","idleCap":0.4,"segments":[{"srcStart":0,"srcEnd":5,"speed":1,"holdEnd":0}]}"#;
    let composed_text = ops::compose_project(&session, project_body).expect("compose_project");
    let cast_export_path = export::export_cast_path(session.cast_path().unwrap());
    std::fs::write(&cast_export_path, &composed_text).expect("write composed cast");

    for (format, ext) in [
        (ExportFormat::Gif, "gif"),
        (ExportFormat::Mp4, "mp4"),
        (ExportFormat::Webm, "webm"),
    ] {
        let video_path = work.0.join(format!("out.{ext}"));
        let gif_target = if format.needs_ffmpeg() {
            work.0.join(format!("intermediate-{ext}.gif"))
        } else {
            video_path.clone()
        };

        let agg_status = Command::new(&agg_path)
            .args(export::agg_args(&cast_export_path, &gif_target))
            .status()
            .unwrap_or_else(|e| panic!("spawn packaged agg: {e}"));
        assert!(agg_status.success(), "{ext}: packaged agg must succeed");
        assert!(gif_target.exists() && std::fs::metadata(&gif_target).unwrap().len() > 0);

        if format.needs_ffmpeg() {
            let ffmpeg_status = Command::new(&ffmpeg_path)
                .args(export::ffmpeg_args(format, &gif_target, &video_path))
                .status()
                .unwrap_or_else(|e| panic!("spawn packaged ffmpeg: {e}"));
            assert!(
                ffmpeg_status.success(),
                "{ext}: packaged ffmpeg must succeed"
            );
        }

        assert!(
            video_path.exists(),
            "{ext}: packaged sidecars must produce {}",
            video_path.display()
        );
        assert!(
            std::fs::metadata(&video_path).unwrap().len() > 0,
            "{ext}: output must be non-empty"
        );
    }

    let _ = std::fs::remove_file(&cast_export_path);
}
