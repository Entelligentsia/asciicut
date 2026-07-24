# Building asciicut installers (CASTCU-S2-T06)

This document is the single source of truth for producing the three
installable packages SPEC.md §7 calls for: **macOS** (`.dmg`/`.app`),
**Linux** (`.deb` + AppImage), **Windows** (`.msi`/`.exe`). Every step below
is exactly what `.github/workflows/desktop-release.yml` runs and what
`scripts/smoke-packaged-build.sh` exercises on Linux — nothing extra, nothing
hidden (AC#6).

S2 ships **unsigned**. See [`OPENING_UNSIGNED.md`](OPENING_UNSIGNED.md) for
how to open an unsigned build on each platform, and its "Signing follow-up"
note for what a signed release would need.

## Prerequisites (all platforms)

- Rust stable (this repo builds with rustc 1.96+) and the matching `cargo`.
- Node.js 20+ / npm (builds the SolidJS SPA and drives the Tauri CLI via
  `web/package.json`'s `devDependency` on `@tauri-apps/cli`).
- A `asciicut-core` wasm target for the SPA's own in-browser plumbing:
  `rustup target add wasm32-unknown-unknown` (the `build:desktop` npm script
  builds this automatically, see below).

## Prerequisites (per platform)

- **Linux** — WebKitGTK 4.1 + GTK3 dev headers and `dpkg`/`fakeroot` for the
  `.deb`, plus whatever `tauri-bundler` needs for AppImage (this repo's Linux
  builds already had `dpkg-deb`, `Xvfb`, `wmctrl`, `xwininfo` available — the
  first two are what packaging/smoke-testing need; the latter two are for the
  window-launch smoke only). No `rpmbuild` is required — AC#1 only asks for
  AppImage and/or `.deb`, and this repo does not build `.rpm`.
- **macOS** — Xcode Command Line Tools (`xcode-select --install`); `dmg`/`.app`
  bundling is native to `tauri-bundler` on macOS, no extra tooling.
- **Windows** — `tauri-bundler` downloads NSIS automatically on first use to
  produce the `.exe` installer; the `.msi` uses WiX3, also auto-fetched. No
  manual installer-toolchain setup is required.

## 1. Fetch the bundled sidecars

`agg` and `ffmpeg` (CASTCU-S2-T04) are declared as `bundle.externalBin` in
`tauri.conf.json` but are **not committed** to the repo. Fetch them for the
host platform before building:

```bash
cd crates/asciicut-desktop
./sidecars/fetch-sidecars.sh
```

This downloads the versions/sources documented in
[`sidecars/README.md`](sidecars/README.md), verifies checksums where
published, and writes `sidecars/<name>-<target-triple>` — the naming
`tauri-bundler` expects to pick the right binary for the platform it is
packaging. See that README for the full per-platform source matrix
(Linux x86_64/aarch64, Windows x86_64, macOS x86_64/aarch64) and for macOS,
where you must supply a source/build yourself
(`FFMPEG_MACOS_X86_64_URL`/`FFMPEG_MACOS_AARCH64_URL` env vars — no upstream
static-build provider is bundled with the script).

## 2. Build the frontend + native binary + installer(s)

From `crates/asciicut-desktop/` (the Tauri CLI is installed as a `web/`
`devDependency`, invoked via its `tauri` npm script which `cd`s into this
crate):

```bash
# Linux — the two AC#1 formats this repo builds
( cd ../../web && npm run tauri -- build --bundles deb,appimage )

# macOS
( cd ../../web && npm run tauri -- build --bundles dmg,app )

# Windows (msi + nsis; nsis produces the .exe installer)
( cd ../../web && npm run tauri -- build --bundles msi,nsis )
```

`tauri build`'s `beforeBuildCommand` (`npm --prefix ../web run build:desktop`,
declared in `tauri.conf.json`) runs automatically first: it compiles
`asciicut-core` to `wasm32-unknown-unknown`, copies the `.wasm` into the SPA,
and runs `vite build --mode desktop`. You do not need to run these steps
separately.

`tauri.conf.json`'s `bundle.targets` is `"all"`; the `--bundles` flag above
narrows each platform's build to exactly the formats AC#1 asks for so no
platform needs tooling beyond what is listed above.

### Where the installers land

```
crates/asciicut-desktop/target/release/bundle/
  deb/asciicut_<version>_amd64.deb
  appimage/asciicut_<version>_amd64.AppImage
  dmg/asciicut_<version>_<arch>.dmg
  macos/asciicut.app
  msi/asciicut_<version>_<arch>_en-US.msi
  nsis/asciicut_<version>_<arch>-setup.exe
```

## 3. Verify the build

- **Linux (this sandbox, run for real):**
  `./scripts/smoke-packaged-build.sh` — builds (unless `--skip-build` is
  passed to reuse an existing bundle), then:
  1. extracts the `.deb` with `dpkg-deb -x` and confirms
     `asciicut-desktop`/`agg`/`ffmpeg` are all present and executable next to
     each other (AC#4 — this is where the installer actually places them, not
     a `resource_dir/sidecars/` subfolder — see `src/sidecars.rs`'s module
     doc for why that distinction matters);
  2. launches the extracted binary under a throwaway Xvfb display with
     `samples/sample.cast` and confirms the native window opens with the
     correct title (`xwininfo`, not `wmctrl` — no window manager runs under
     Xvfb, matching CASTCU-S2-T01/T02's own smoke pattern) — AC#2's
     launch+open-cast half;
  3. runs `tests/packaged_build_smoke.rs`'s `--ignored` tests: the production
     `sidecars::resolve_checked` resolver against both the extracted `.deb`
     layout and the AppImage's already-unpacked `asciicut.AppDir` (no FUSE
     mount needed), plus a real compose → `agg` → `ffmpeg` export producing
     genuine `.gif`/`.mp4`/`.webm` files from the **exact bundled binaries** —
     AC#2's edit+export half and AC#4's "resolved correctly at runtime" half.
- **macOS / Windows:** verified by `.github/workflows/desktop-release.yml`'s
  `macos-latest`/`windows-latest` matrix legs, which run the same
  fetch-sidecars → build → (bundle-specific) smoke sequence described here.
  This sandbox cannot execute a `macos-latest`/`windows-latest` runner
  directly — that gap is recorded honestly in `PROGRESS.md`, not glossed
  over; the workflow YAML's correctness is established by the Linux leg of
  the same matrix succeeding end-to-end plus manual review, not by claiming
  execution that did not happen here.

## 4. `fmt`/clippy (AC#7)

This crate is a **detached workspace** (`crates/asciicut-desktop/Cargo.toml`
carries its own `[workspace]` table, excluded from the root virtual
workspace — see that file's header comment), so it needs its own
`--manifest-path` invocation in addition to the root gate:

```bash
cargo fmt --all --manifest-path crates/asciicut-desktop/Cargo.toml -- --check
cargo clippy --all-targets --all-features --manifest-path crates/asciicut-desktop/Cargo.toml -- -D warnings
```

## CI (macOS/Windows path)

`.github/workflows/desktop-release.yml` is a `workflow_dispatch` (manually
triggered) matrix over `ubuntu-latest`/`macos-latest`/`windows-latest` that
runs exactly the steps documented above per platform: checkout, Rust + Node
setup, Linux system deps on the `ubuntu-latest` leg, `fmt`/`clippy` scoped to
this crate's manifest path, `sidecars/fetch-sidecars.sh`, `tauri build
--bundles ...` for that platform's formats, and `upload-artifact` for the
produced installer(s). It is deliberately **not** part of `.github/workflows/ci.yml`
(the root workspace's per-push/PR gate) — a three-OS matrix that downloads a
full `ffmpeg` static build is a release-shaped job, not a per-commit one, so
it runs on demand instead of adding cost to every push.
