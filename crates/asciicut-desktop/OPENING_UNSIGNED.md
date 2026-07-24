# Opening an unsigned asciicut build (S2)

S2 ships **unsigned** on every platform (AC#5) — no Apple Developer ID +
notarization, no Windows code-signing certificate. Both cost money and are
out of scope for S2 (see "Signing follow-up" below). Each OS's own
unsigned-app gatekeeper will flag the app the first time you open it; here is
how to get past that on each platform.

## macOS — Gatekeeper

Gatekeeper blocks an unsigned/unnotarized `.app` from launching via a normal
double-click ("asciicut.app is damaged and can't be opened" or "cannot be
opened because the developer cannot be verified").

**Option A — right-click Open (recommended, no terminal):**

1. In Finder, locate `asciicut.app` (from the `.dmg`, after dragging it to
   `Applications`, or the raw `.app` bundle).
2. **Right-click → Open** (not a normal double-click).
3. A dialog still warns it's from an unidentified developer, but now offers
   an **Open** button — click it. macOS remembers this choice for future
   launches of this exact binary.

**Option B — clear the quarantine attribute (terminal):**

```bash
xattr -dr com.apple.quarantine /Applications/asciicut.app
```

This removes the `com.apple.quarantine` flag macOS attaches to anything
downloaded from the internet (including a `.dmg`), which is what triggers the
Gatekeeper check in the first place.

## Windows — SmartScreen

Windows Defender SmartScreen shows "Windows protected your PC" when running
an installer with no recognized code-signing certificate.

1. On the SmartScreen dialog, click **More info**.
2. A **Run anyway** button appears — click it to proceed with the `.msi`/`.exe`
   installer.

This appears once per unsigned binary per machine; subsequent runs of the
same installed app do not re-trigger it (SmartScreen's reputation check is
per-binary-hash, tied to the installer file, not the installed app).

## Linux — AppImage / `.deb`

Linux has no OS-level unsigned-binary gatekeeper equivalent to Gatekeeper or
SmartScreen — trust here is about file permissions and package sourcing, not
a security prompt to click through.

- **AppImage:** make it executable, then run it directly:

  ```bash
  chmod +x asciicut_<version>_amd64.AppImage
  ./asciicut_<version>_amd64.AppImage
  ```

  Some desktop environments' file managers require enabling "Allow executing
  file as program" in the file's Properties → Permissions tab instead of a
  terminal `chmod`.

- **`.deb`:** install with `dpkg` (or your distro's package manager), which
  itself has no unsigned-package block for a locally-provided `.deb` (unlike
  an APT repository, which does check signatures — this is a direct local
  install, not a repo add):

  ```bash
  sudo dpkg -i asciicut_<version>_amd64.deb
  # if dpkg reports missing dependencies:
  sudo apt-get install -f
  ```

  `dpkg -i` runs the package's postinst scripts with root privileges — only
  install `.deb` files from a source you trust, exactly as with any other
  local package install.

## Signing follow-up (not attempted in S2)

Recorded here as a documented follow-up, not a blocker, per the task prompt:

- **macOS** — requires an active Apple Developer Program membership (paid,
  annual) to get a Developer ID Application certificate, plus running the
  built `.app` through `notarytool` (Apple's notarization service) before
  distribution. `tauri-bundler` has built-in support for both once the
  certificate/Apple ID credentials are available as CI secrets.
- **Windows** — requires a code-signing certificate from a recognized CA
  (also a paid, ongoing cost) and signing the `.msi`/`.exe` with
  `signtool`/`osslsigncode` (or EV-cert-based cloud signing to avoid
  SmartScreen's reputation cold-start). `tauri-bundler` supports wiring a
  signing command into the bundle step once a certificate exists.
- **Linux** — no OS-level signing requirement exists for `.deb`/AppImage in
  the way macOS/Windows require it; GPG-signing an APT repository (if one is
  ever stood up) is the closest analog and is unrelated to this document's
  scope.

Neither certificate currently exists for this project; provisioning them is
a business/cost decision outside S2's scope, tracked here so it is not
forgotten rather than silently skipped.
