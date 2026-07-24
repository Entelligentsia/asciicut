#!/usr/bin/env bash
# Fetch and stage per-platform agg + ffmpeg sidecars for the asciicut desktop app.
# Run from crates/asciicut-desktop/.
#
# Usage:
#   ./sidecars/fetch-sidecars.sh              # fetch for the current host
#   SIDECAR_AGG_VERSION=1.5.0 ./sidecars/fetch-sidecars.sh   # override version
#
# The script is safe to re-run: existing files are replaced. It writes binaries
# into sidecars/ with the exact target-triple filenames Tauri expects.

set -euo pipefail

cd "$(dirname "$0")/.."

SIDEARS_DIR="sidecars"
mkdir -p "$SIDEARS_DIR"

# ─────────────────────────────────────────────────────────────────────────────
# Configuration: versions and source URLs. Update these when bumping sidecars.
# ─────────────────────────────────────────────────────────────────────────────
SIDECAR_AGG_VERSION="${SIDECAR_AGG_VERSION:-1.5.0}"
# asciinema/agg release archive naming: agg-<version>-<triple>.tar.gz
AGG_BASE_URL="https://github.com/asciinema/agg/releases/download/v${SIDECAR_AGG_VERSION}"

# BtbN "latest" auto-build URLs. These track a rolling "latest" tag; pin to a
# release asset URL in CI if reproducibility matters more than freshness.
FFMPEG_LINUX_X86_64_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
FFMPEG_LINUX_AARCH64_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz"
FFMPEG_WIN_X86_64_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"

# macOS builds are not provided by BtbN; the project must build or source them.
# Set FFMPEG_MACOS_X86_64_URL / FFMPEG_MACOS_AARCH64_URL externally to override.
FFMPEG_MACOS_X86_64_URL="${FFMPEG_MACOS_X86_64_URL:-}"
FFMPEG_MACOS_AARCH64_URL="${FFMPEG_MACOS_AARCH64_URL:-}"

# ─────────────────────────────────────────────────────────────────────────────
# Platform detection
# ─────────────────────────────────────────────────────────────────────────────
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux)   OS=linux ;;
  darwin)  OS=macos ;;
  mingw*|msys*|cygwin*) OS=windows ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  arm64|aarch64) ARCH=aarch64 ;;
esac

case "${OS}-${ARCH}" in
  linux-x86_64)     TRIPLE="x86_64-unknown-linux-gnu" ;;
  linux-aarch64)    TRIPLE="aarch64-unknown-linux-gnu" ;;
  macos-x86_64)     TRIPLE="x86_64-apple-darwin" ;;
  macos-aarch64)    TRIPLE="aarch64-apple-darwin" ;;
  windows-x86_64)   TRIPLE="x86_64-pc-windows-msvc" ;;
  windows-aarch64)  TRIPLE="aarch64-pc-windows-msvc" ;;
  *) echo "unsupported platform: $OS/$ARCH" >&2; exit 1 ;;
esac

echo "Fetching sidecars for $TRIPLE ..."

# ─────────────────────────────────────────────────────────────────────────────
# Fetch helpers
# ─────────────────────────────────────────────────────────────────────────────
download() {
  local url="$1"
  local out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --tries=3 "$url" -O "$out"
  else
    echo "need curl or wget to fetch sidecars" >&2
    exit 1
  fi
}

fetch_agg() {
  local triple="$1"
  local out_bin="$SIDEARS_DIR/agg-${triple}"

  # Try the single-binary release artifact first (newer agg releases).
  local url="${AGG_BASE_URL}/agg-${triple}"
  echo "  agg: $url"
  if download "$url" "$out_bin"; then
    chmod +x "$out_bin" 2>/dev/null || true
    return 0
  fi

  # Fall back to the .tar.gz archive naming used by some releases.
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  url="${AGG_BASE_URL}/agg-v${SIDECAR_AGG_VERSION}-${triple}.tar.gz"
  echo "  agg (fallback archive): $url"
  download "$url" "$tmp/agg.tar.gz"
  tar -xzf "$tmp/agg.tar.gz" -C "$tmp"
  # The archive usually contains a single `agg` binary.
  find "$tmp" -name agg -type f -exec cp {} "$out_bin" \;
  chmod +x "$out_bin"
}

fetch_ffmpeg() {
  local triple="$1"
  local out_bin="$SIDEARS_DIR/ffmpeg-${triple}"

  local url=""
  case "$triple" in
    x86_64-unknown-linux-gnu)  url="$FFMPEG_LINUX_X86_64_URL" ;;
    aarch64-unknown-linux-gnu) url="$FFMPEG_LINUX_AARCH64_URL" ;;
    x86_64-pc-windows-msvc)    url="$FFMPEG_WIN_X86_64_URL" ;;
    x86_64-apple-darwin)        url="$FFMPEG_MACOS_X86_64_URL" ;;
    aarch64-apple-darwin)      url="$FFMPEG_MACOS_AARCH64_URL" ;;
  esac

  if [ -z "$url" ]; then
    echo "  ffmpeg: no URL configured for $triple; set FFMPEG_${triple//-/_}_URL" >&2
    return 1
  fi

  echo "  ffmpeg: $url"
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  case "$url" in
    *.tar.xz)
      download "$url" "$tmp/ffmpeg.tar.xz"
      tar -xf "$tmp/ffmpeg.tar.xz" -C "$tmp"
      ;;
    *.zip)
      download "$url" "$tmp/ffmpeg.zip"
      unzip -q "$tmp/ffmpeg.zip" -d "$tmp"
      ;;
    *)
      download "$url" "$out_bin"
      chmod +x "$out_bin" 2>/dev/null || true
      return 0
      ;;
  esac

  find "$tmp" -name 'ffmpeg' -o -name 'ffmpeg.exe' | head -n1 | while read -r bin; do
    cp "$bin" "$out_bin"
  done
  chmod +x "$out_bin" 2>/dev/null || true
}

# ─────────────────────────────────────────────────────────────────────────────
# Run the fetches
# ─────────────────────────────────────────────────────────────────────────────
fetch_agg "$TRIPLE" || {
  echo "ERROR: failed to fetch agg for $TRIPLE" >&2
  exit 1
}

fetch_ffmpeg "$TRIPLE" || {
  echo "ERROR: failed to fetch ffmpeg for $TRIPLE" >&2
  exit 1
}

# ─────────────────────────────────────────────────────────────────────────────
# Health check + attribution stub
# ─────────────────────────────────────────────────────────────────────────────
AGG_OUT="$SIDEARS_DIR/agg-${TRIPLE}"
FFMPEG_OUT="$SIDEARS_DIR/ffmpeg-${TRIPLE}"

echo "Staged:"
ls -lh "$AGG_OUT" "$FFMPEG_OUT"

cat > "$SIDEARS_DIR/ATTRIBUTION.md" <<EOF
# asciicut sidecar attribution

Generated by fetch-sidecars.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ).

- agg v${SIDECAR_AGG_VERSION} — GPL v3 — https://github.com/asciinema/agg
- ffmpeg — source: ${FFMPEG_LINUX_X86_64_URL} (linux x86_64 example) —
  see README.md for the full per-platform source matrix and license details.
EOF

echo "Sidecars ready."
