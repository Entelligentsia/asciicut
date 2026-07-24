# Distribution channels

How asciicut reaches users, and how to publish each channel. The desktop
installers are produced by [`.github/workflows/desktop-release.yml`](../.github/workflows/desktop-release.yml)
and attached to each GitHub Release; the manifests below point at those assets.

Regenerate every manifest for a release from its asset checksums:

```sh
packaging/generate.sh v0.1.0     # or omit the tag for the latest release
```

---

## Desktop app (GUI installers)

These distribute the Tauri app built on each release.

### macOS — Homebrew Cask  ✅ ready

Source of truth: [`homebrew/asciicut.rb`](homebrew/asciicut.rb) (arm64 `.dmg`).

**Publish** (one-time tap, then one commit per release):

1. Create a public repo **`Entelligentsia/homebrew-tap`**.
2. Copy the cask into it: `Casks/asciicut.rb`.
3. Users install with:
   ```sh
   brew install --cask --no-quarantine entelligentsia/tap/asciicut
   ```
   `--no-quarantine` skips Gatekeeper on the unsigned build (see
   [`OPENING_UNSIGNED.md`](../crates/asciicut-desktop/OPENING_UNSIGNED.md)).

> The macOS CI runner is Apple Silicon, so only an **arm64** `.dmg` is built.
> Add an `x86_64` macOS leg to the release workflow to also serve Intel Macs,
> then split the cask with `on_arm` / `on_intel`.

### Windows — winget  🔜 needs submission

winget manifests are best generated and validated with **wingetcreate** rather
than hand-authored. `generate.sh` prints the exact command and the installer
SHA-256. Then:

```sh
wingetcreate submit --token <gh-token> \
  'https://github.com/Entelligentsia/asciicut/releases/download/v0.1.0/asciicut_0.1.0_x64_en-US.msi' \
  --version 0.1.0 --id Entelligentsia.asciicut
```

This opens a PR against **microsoft/winget-pkgs**; once merged, users run
`winget install Entelligentsia.asciicut`. (The `.msi` is the WiX installer; the
`x64-setup.exe` is the NSIS alternative.)

### Linux — direct + AppImageHub

The `.deb` and `.AppImage` on the release are the install path today (see the
README). Optional later: an **AppImageHub** listing for the AppImage, and/or a
signed **apt repository** for the `.deb`.

### Windows — Scoop  ⚠️ not a fit yet

Scoop expects a **portable** app (a zip of the unpacked binary), not an
installer. Our release ships `.msi`/`.exe` installers only. To add Scoop, have
the release also produce a portable Tauri build (zip) and add a bucket repo
`Entelligentsia/scoop-bucket`.

---

## CLI / `npx asciicut`  ✅ configured (cargo-dist)

The `asciicut` binary is also a CLI + local web server (`asciicut file.cast`).
It's distributed via **cargo-dist** (config in [`dist-workspace.toml`](../dist-workspace.toml),
workflow in [`.github/workflows/release.yml`](../.github/workflows/release.yml)),
which on a version tag builds the binary for every target and produces:

- **npm / `npx asciicut`** — the flagship one-liner
- **Homebrew formula** (the CLI binary, published to the same `homebrew-tap`;
  distinct from the GUI cask above)
- **`curl | sh`** shell installer (`asciicut-installer.sh`)
- **cargo-binstall**-compatible tarballs + `sha256.sum`

Because `asciicut` embeds the SolidJS SPA (rust-embed), the release workflow
builds the web bundle first — see [`.github/workflows/build-setup.yml`](../.github/workflows/build-setup.yml).

**How a release flows** (one tag drives everything):

```sh
# bump version in Cargo.toml / tauri.conf.json, then:
git tag v0.2.0 && git push origin v0.2.0
```

1. `release.yml` (cargo-dist) builds the CLI, **creates the GitHub Release**,
   and publishes the npm package + Homebrew formula.
2. Publishing the release fires `release: published`, so `desktop-release.yml`
   appends the GUI installers (`.dmg`/`.msi`/`.AppImage`/`.deb`) + attestations.

**To activate**, provision once:

- **`NPM_TOKEN`** repo secret — an npm automation token (npm publish).
- **`Entelligentsia/homebrew-tap`** repo + **`HOMEBREW_TAP_TOKEN`** secret — a PAT
  with write access to the tap (a cross-repo push; `GITHUB_TOKEN` can't do it).
- Claim the unscoped **`asciicut`** name on npm (or set an `npm-scope` in
  `dist-workspace.toml` for a scoped `npx @scope/asciicut`).

Validate end-to-end on a throwaway pre-release tag (e.g. `v0.0.0-rc1`) before the
first real one — the dist ⇄ desktop-release coordination hasn't been run yet.

---

## Checklist for a new release

1. Cut the release → `desktop-release.yml` builds + attaches the installers,
   `SHA256SUMS-<OS>.txt`, and provenance attestations.
2. `packaging/generate.sh <tag>` — refresh the cask + print winget values.
3. Commit the cask, sync it to `homebrew-tap`, and `wingetcreate submit` the msi.
4. (Once set up) cargo-dist publishes the npm/brew-formula/shell channels.
