#!/usr/bin/env bash
# Packaged-build smoke test (CASTCU-S2-T06). Run from crates/asciicut-desktop/.
#
# Proves the REAL `tauri build`-produced Linux installers work end to end —
# not the dev/debug binary earlier tasks' smoke tests use:
#
#   1. Build the .deb and AppImage bundles for real (`tauri build --bundles
#      deb,appimage`), with the fetched sidecars already staged (run
#      sidecars/fetch-sidecars.sh first if `sidecars/agg-*`/`sidecars/ffmpeg-*`
#      are missing — this script checks and fails fast with that instruction).
#   2. Launch the extracted .deb's own `asciicut-desktop <sample.cast>` under a
#      throwaway Xvfb display and confirm the native window opens with the
#      correct title (xwininfo — no window manager is running under Xvfb, so
#      wmctrl's window list is unusable here; T01/T02 established this same
#      pattern).
#   3. Run `tests/packaged_build_smoke.rs`'s `--ignored` tests: the production
#      sidecar resolver (`sidecars::resolve_checked`) against both the
#      extracted .deb layout and the AppImage's unpacked `asciicut.AppDir`, and
#      a real compose -> agg -> ffmpeg export using the exact bundled binaries.
#
# Usage:
#   ./scripts/smoke-packaged-build.sh              # build + smoke everything
#   ./scripts/smoke-packaged-build.sh --skip-build  # smoke an existing bundle
#
# Exit non-zero on any failure. Safe to re-run.

set -euo pipefail

cd "$(dirname "$0")/.."
CRATE_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

SKIP_BUILD=0
if [ "${1:-}" = "--skip-build" ]; then
  SKIP_BUILD=1
fi

log() { echo "[smoke-packaged-build] $*"; }

TRIPLE_GUESS="$(uname -m)-unknown-linux-gnu"
if [ ! -f "sidecars/agg-${TRIPLE_GUESS}" ] || [ ! -f "sidecars/ffmpeg-${TRIPLE_GUESS}" ]; then
  echo "ERROR: sidecars/agg-${TRIPLE_GUESS} / ffmpeg-${TRIPLE_GUESS} not found." >&2
  echo "Run ./sidecars/fetch-sidecars.sh first (see sidecars/README.md)." >&2
  exit 1
fi

# ─────────────────────────────────────────────────────────────────────────────
# 1. Build the real installers
# ─────────────────────────────────────────────────────────────────────────────
if [ "$SKIP_BUILD" -eq 0 ]; then
  log "Building .deb + AppImage via tauri build --bundles deb,appimage ..."
  ( cd "$REPO_ROOT/web" && npm run tauri -- build --bundles deb,appimage )
else
  log "--skip-build passed; using whatever is already under target/release/bundle/"
fi

DEB_PATH="$(find target/release/bundle/deb -maxdepth 1 -name '*.deb' | head -n1)"
APPDIR="target/release/bundle/appimage/asciicut.AppDir"
if [ -z "$DEB_PATH" ] || [ ! -f "$DEB_PATH" ]; then
  echo "ERROR: no .deb found under target/release/bundle/deb/" >&2
  exit 1
fi
if [ ! -d "$APPDIR" ]; then
  echo "ERROR: $APPDIR not found (AppImage bundle step did not run)" >&2
  exit 1
fi
log "Found bundles: $DEB_PATH ; $APPDIR"

# ─────────────────────────────────────────────────────────────────────────────
# 2. Extract the .deb and launch it under Xvfb — confirm the native window
#    opens with the correct title (T01's pattern: no WM under Xvfb, so
#    xwininfo -root -tree, not wmctrl).
# ─────────────────────────────────────────────────────────────────────────────
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

log "Extracting $DEB_PATH ..."
dpkg-deb -x "$DEB_PATH" "$SCRATCH/deb-extracted"
BIN="$SCRATCH/deb-extracted/usr/bin/asciicut-desktop"
if [ ! -x "$BIN" ]; then
  echo "ERROR: $BIN missing or not executable after dpkg-deb -x" >&2
  exit 1
fi
for sidecar in agg ffmpeg; do
  if [ ! -x "$SCRATCH/deb-extracted/usr/bin/$sidecar" ]; then
    echo "ERROR: $sidecar not bundled next to asciicut-desktop in the .deb — AC#4 violated" >&2
    exit 1
  fi
done
log "Confirmed: asciicut-desktop, agg, ffmpeg all present+executable in the extracted .deb"

if ! command -v Xvfb >/dev/null 2>&1 || ! command -v xwininfo >/dev/null 2>&1; then
  echo "ERROR: Xvfb and xwininfo are required for the window-launch smoke" >&2
  exit 1
fi

DISPLAY_NUM=":$((90 + RANDOM % 400))"
log "Starting Xvfb on $DISPLAY_NUM ..."
Xvfb "$DISPLAY_NUM" -screen 0 1280x800x24 >"$SCRATCH/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

SAMPLE_CAST="$REPO_ROOT/samples/sample.cast"
log "Launching packaged binary with $SAMPLE_CAST under $DISPLAY_NUM ..."
DISPLAY="$DISPLAY_NUM" "$BIN" "$SAMPLE_CAST" >"$SCRATCH/app.log" 2>&1 &
APP_PID=$!

cleanup_app() {
  kill "$APP_PID" >/dev/null 2>&1 || true
  sleep 1
  kill "$XVFB_PID" >/dev/null 2>&1 || true
}

# Poll for the titled window rather than checking once: webview window init
# under headless Xvfb takes noticeably longer on a shared CI runner than on a
# dev box, and the window first appears with its binary-name placeholder before
# the configured title is applied. Wait up to ~30s, re-checking each second and
# bailing early if the process dies.
WIN_INFO=""
WIN_OK=0
for _ in $(seq 1 30); do
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "ERROR: packaged binary exited early. Log:" >&2
    cat "$SCRATCH/app.log" >&2
    cleanup_app
    exit 1
  fi
  WIN_INFO="$(DISPLAY="$DISPLAY_NUM" xwininfo -root -tree 2>/dev/null || true)"
  if echo "$WIN_INFO" | grep -q 'asciicut — the cutting room'; then
    WIN_OK=1
    break
  fi
  sleep 1
done

if [ "$WIN_OK" = 1 ]; then
  log "Native window confirmed: asciicut — the cutting room"
else
  echo "ERROR: expected window title not found after ~30s. xwininfo output:" >&2
  echo "$WIN_INFO" >&2
  echo "--- app.log ---" >&2
  cat "$SCRATCH/app.log" >&2
  cleanup_app
  exit 1
fi

cleanup_app
log "Window-launch smoke passed."

# ─────────────────────────────────────────────────────────────────────────────
# 3. Resolver + real compose/export smoke (tests/packaged_build_smoke.rs)
# ─────────────────────────────────────────────────────────────────────────────
log "Running packaged_build_smoke.rs (resolver + real agg/ffmpeg export) ..."
cargo test --manifest-path "$CRATE_DIR/Cargo.toml" --test packaged_build_smoke -- --ignored --nocapture

log "All packaged-build smoke checks passed."
