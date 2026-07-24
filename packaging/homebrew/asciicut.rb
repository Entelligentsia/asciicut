# Homebrew Cask for the asciicut desktop app (macOS).
#
# Source of truth lives here; publish by copying this file into the Casks/
# directory of the Entelligentsia/homebrew-tap repo (see packaging/README.md).
# Regenerate for a new release with: packaging/generate.sh <tag>
#
# NOTE: the release currently ships an Apple-Silicon (arm64) .dmg only — the CI
# macOS runner is arm64. Add an x86_64 leg to desktop-release.yml to also serve
# Intel Macs, then extend this cask with an on_intel/on_arm split.
cask "asciicut" do
  version "0.1.0"
  sha256 "f0f29484a198f9b7965bbacfa5080642f263ab4af7fde887b054a1a43d318bc7"

  url "https://github.com/Entelligentsia/asciicut/releases/download/v#{version}/asciicut_#{version}_aarch64.dmg"
  name "asciicut"
  desc "Visual editor for asciinema terminal recordings"
  homepage "https://github.com/Entelligentsia/asciicut"

  depends_on arch: :arm64

  app "asciicut.app"

  # The app is unsigned/unnotarized (see OPENING_UNSIGNED.md). Users install with
  #   brew install --cask --no-quarantine entelligentsia/tap/asciicut
  # to skip Gatekeeper, or right-click -> Open once after a normal install.

  zap trash: [
    "~/Library/Application Support/com.entelligentsia.asciicut",
    "~/Library/Caches/com.entelligentsia.asciicut",
    "~/Library/Preferences/com.entelligentsia.asciicut.plist",
    "~/Library/Saved Application State/com.entelligentsia.asciicut.savedState",
  ]
end
