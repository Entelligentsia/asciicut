# asciicut sidecars — `agg` + `ffmpeg`

This directory holds the **per-platform binaries** bundled with the asciicut desktop app via Tauri v2 `bundle.externalBin`. The binaries themselves are **not committed**; they are fetched by `fetch-sidecars.sh` at build/packaging time and ignored by git.

## Bundled tools

| Sidecar | License | Purpose | Source |
|---|---|---|---|
| `agg` | GPL v3 | Convert a `.cast` recording into an animated `.gif` | [asciinema/agg](https://github.com/asciinema/agg) releases |
| `ffmpeg` | GPL v3 / LGPL depending on build | Encode composed `.cast` exports to `.mp4`, `.webm`, or `.gif` | Redistributable static builds (see matrix below) |

## Why separate processes keep asciicut MIT

Both `agg` and the chosen `ffmpeg` builds are GPL-family. asciicut invokes them as **separate command-line processes** through `tauri-plugin-shell` — they are not linked into the asciicut binary and do not run in the same address space. That separation is the standard interpretation that keeps the MIT-licensed asciicut application itself outside GPL copyleft, while the installer must still include the sidecar license notices and source pointers below.

## ffmpeg codec scope

The bundled `ffmpeg` is intentionally **scoped** to the codecs asciicut needs for export:

- `.mp4` — libx264 and libx265
- `.webm` — libvpx-vp9
- `.gif` — built-in GIF encoder (used directly or via `agg`)

Keeping the codec set narrow lets us choose smaller redistributable static builds and documents exactly what we ship.

## Per-platform source matrix

| Platform | Target triple | Source / build | Notes |
|---|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | [BtbN GPL static builds](https://github.com/BtbN/FFmpeg-Builds) | Pick the `gpl` variant; verify it includes libx264, libx265, libvpx |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | BtbN GPL static builds | Same variant as x86_64 |
| Windows x86_64 | `x86_64-pc-windows-msvc` | BtbN GPL static builds | `.exe` suffix is added by the fetch script |
| macOS x86_64 | `x86_64-apple-darwin` | [OSXExperts GPL static build](https://www.osxexperts.net/) — `ffmpeg80intel.zip` | Default in `fetch-sidecars.sh` (SHA-256 pinned); override with `FFMPEG_MACOS_X86_64_URL` |
| macOS aarch64 | `aarch64-apple-darwin` | [OSXExperts GPL static build](https://www.osxexperts.net/) — `ffmpeg81arm.zip` | Default in `fetch-sidecars.sh` (SHA-256 pinned); override with `FFMPEG_MACOS_AARCH64_URL` |

The **Linux x86_64** path is proven first on the development box (CASTCU-S2-T04). macOS and Windows sourcing are verified during packaged-build testing in CASTCU-S2-T06.

### macOS ffmpeg default sources

`fetch-sidecars.sh` defaults the two macOS ffmpeg URLs to OSXExperts' GPL static
builds (which include libx264, libx265, and libvpx):

| Triple | Default URL |
|---|---|
| `x86_64-apple-darwin` | `https://www.osxexperts.net/ffmpeg80intel.zip` |
| `aarch64-apple-darwin` | `https://www.osxexperts.net/ffmpeg81arm.zip` |

Like the BtbN Linux/Windows "latest" builds above, OSXExperts serves a rolling
"latest for the major" under a **stable filename**, so the contents change when
they republish. The script therefore does **not** pin a checksum by default (a
pin would break the build on every upstream rebuild). To harden a specific build
for reproducibility, set `FFMPEG_MACOS_X86_64_SHA256` / `FFMPEG_MACOS_AARCH64_SHA256`
(env or CI secret) — when non-empty, the download is verified against it.

Override the source entirely with `FFMPEG_MACOS_X86_64_URL` /
`FFMPEG_MACOS_AARCH64_URL`. An alternative Intel-only source with **versioned,
immutable** URLs (so a pinned checksum stays valid) is
[evermeet.cx](https://evermeet.cx/ffmpeg/) — use a `ffmpeg-<ver>.zip` URL so the
script's `.zip` extractor matches.

## `agg` source

Download the `agg` release archive matching the target from the [asciinema/agg releases page](https://github.com/asciinema/agg/releases). The fetch script extracts the single `agg` binary and renames it to `agg-<target-triple>`.

## Fetch script

Run from the `crates/asciicut-desktop/` directory:

```bash
./sidecars/fetch-sidecars.sh
```

The script detects the host OS/arch, downloads the documented versions, verifies SHA-256 checksums where published, and renames the binaries into the exact `sidecars/<name>-<target-triple>` filenames Tauri expects. It is safe to re-run; existing files are replaced.

## Size budget

The S2 size budget for the two sidecars combined is **≤ 240 MB uncompressed per platform** (≈ 120 MB each for `agg` and `ffmpeg` static builds). Actual sizes must be recorded here after the first fetch:

| Platform | agg size | ffmpeg size | total | recorded |
|---|---|---|---|---|
| Linux x86_64 | 8.5 MB | 145.6 MB | 154.1 MB | 2026-07-24 (CASTCU-S2-T06, real `fetch-sidecars.sh` run) |
| Linux aarch64 | _pending fetch_ | _pending fetch_ | _pending fetch_ | — |
| Windows x86_64 | _pending fetch_ | _pending fetch_ | _pending fetch_ | — (CI matrix, not run from this sandbox) |
| macOS x86_64 | _pending fetch_ | _pending fetch_ | _pending fetch_ | — (CI matrix; also needs `FFMPEG_MACOS_X86_64_URL` supplied, see below) |
| macOS aarch64 | _pending fetch_ | _pending fetch_ | _pending fetch_ | — (CI matrix; also needs `FFMPEG_MACOS_AARCH64_URL` supplied, see below) |

Packaging strips debug symbols and may compress the installer, but the uncompressed bundle must stay within this budget. (BtbN's Linux/Windows URLs track a rolling "latest" tag, so the exact byte counts drift release to release — both are comfortably inside the ≤240MB budget with room to spare.)

**Real Linux installer sizes** (CASTCU-S2-T06, `tauri build --bundles deb,appimage` in this sandbox, sidecars included and confirmed bundled — see `crates/asciicut-desktop/BUILD.md` / `scripts/smoke-packaged-build.sh`):

| Installer | Size |
|---|---|
| `asciicut_0.1.0_amd64.deb` | 64 MB |
| `asciicut_0.1.0_amd64.AppImage` | 130 MB |

(Compressed installer sizes are smaller than the 154.1 MB uncompressed sidecar total above because both formats compress their payload — `.deb`'s `data.tar.gz` and the AppImage's SquashFS.)

## License attribution in the installer

Each installer/releasable archive must reproduce:

- `agg` — a copy of or link to [GPL v3](https://www.gnu.org/licenses/gpl-3.0.html) and the [asciinema/agg source repository](https://github.com/asciinema/agg).
- `ffmpeg` — the license file shipped with the static build (GPL or LGPL as appropriate), plus a source pointer to the exact build URL or FFmpeg commit used.

The fetch script writes a `sidecars/ATTRIBUTION.md` next to the binaries at fetch time so packaging can include it automatically.

## Runtime discovery

At runtime Tauri v2 places the sidecar files **next to the app binary itself** — e.g. a Linux `.deb`/AppImage install ends up with `usr/bin/asciicut-desktop`, `usr/bin/agg`, `usr/bin/ffmpeg` all as siblings, stripped of the target-triple suffix. This is *not* a `resource_dir/sidecars/` subfolder; confirmed by inspecting the real bundle output during CASTCU-S2-T06 (a previous version of this doc, and of the resolver itself, assumed the resource-dir path — corrected once a genuinely packaged `.deb` proved neither dev mode nor `tauri-bundler` ever populate it). The Rust resolver in `src/sidecars.rs` (`resolve`/`resolve_checked`) matches this exactly: it resolves against the directory containing the running executable (`std::env::current_exe()`'s parent, mirroring `tauri_plugin_shell::Command::sidecar`'s own resolution), bare-named, no `-<triple>` suffix — that suffix only exists on the *source* files this directory's `fetch-sidecars.sh` writes, which `tauri-build` (dev) and `tauri-bundler` (packaged) both strip before the app ever runs. See `src/sidecars.rs`'s module doc for the full story, and `tests/packaged_build_smoke.rs` for the proof against a real built `.deb`/AppImage.

## Adding a new platform

1. Add the `(OS, ARCH) → target-triple` entry in `src/sidecars.rs`.
2. Document the source/build in the matrix above.
3. Add the download + rename step to `fetch-sidecars.sh`.
4. Run the resolver unit tests and the real `--version` smoke command on the target platform.
