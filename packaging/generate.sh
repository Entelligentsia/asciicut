#!/usr/bin/env bash
# Regenerate distribution-channel manifests from a published GitHub release.
#
# Usage:  packaging/generate.sh [tag]        # default: latest release tag
#
# Pulls each installer's SHA-256 straight from the GitHub release API (no
# downloads) and rewrites the Homebrew cask + prints the values winget/Scoop
# need. Run after a release's installers have finished uploading.
set -euo pipefail

REPO="Entelligentsia/asciicut"
HERE="$(cd "$(dirname "$0")" && pwd)"

tag="${1:-$(gh release view --repo "$REPO" --json tagName -q .tagName)}"
version="${tag#v}"
echo "Generating channel manifests for $REPO $tag (version $version)"

# name -> sha256 map from the release assets
digest() {
  gh api "repos/$REPO/releases/tags/$tag" \
    --jq ".assets[] | select(.name==\"$1\") | (.digest // \"\") | sub(\"sha256:\";\"\")"
}

dmg_sha="$(digest "asciicut_${version}_aarch64.dmg")"
msi="asciicut_${version}_x64_en-US.msi";   msi_sha="$(digest "$msi")"
exe="asciicut_${version}_x64-setup.exe";   exe_sha="$(digest "$exe")"

[ -n "$dmg_sha" ] || { echo "no aarch64 .dmg on $tag yet" >&2; exit 1; }

# ---- Homebrew cask (macOS) --------------------------------------------------
cask="$HERE/homebrew/asciicut.rb"
sed -i.bak -E "s/^  version \".*\"/  version \"$version\"/; s/^  sha256 \".*\"/  sha256 \"$dmg_sha\"/" "$cask"
rm -f "$cask.bak"
echo "  ✓ updated homebrew/asciicut.rb (dmg $dmg_sha)"

# ---- winget (Windows) -------------------------------------------------------
echo
echo "winget — run from any machine with wingetcreate (see README):"
echo "  wingetcreate new \\"
echo "    'https://github.com/$REPO/releases/download/$tag/$msi' \\"
echo "    --version $version --id Entelligentsia.asciicut"
echo "  msi  sha256: ${msi_sha^^}"
echo "  exe  sha256: ${exe_sha^^}"
