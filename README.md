# asciicut

### the visual cutting room for terminal recordings

<p align="center">
  <img src="assets/logo_master_transparent.png" alt="Nibbles the Beaver — the asciicut mascot, in safety goggles holding a giant pair of scissors" width="240" />
</p>

`asciicut` is a visual editor for asciinema `.cast` files. Load a recording, **see
where the dead air is** on an activity timeline, cut it, pace the rest, and export
a tight clip as `.cast`, mp4, webm, or gif.

> Every existing asciicast tool is a blind CLI — `cut --start 60 --end 470` with
> no way to *see* what you're removing. The asciinema project itself says there's
> [no visual editor](https://docs.asciinema.org/faq/) "due to the incremental,
> state-machine based nature of terminal emulators." That's true for editing
> *pixels* — but not for editing *time*. asciicut edits time, and shows you the
> recording while you do it.

**Status:** 🛠 working — the editor and a **native desktop app** ship today: activity
timeline, filmstrip, segment editing with per-segment speed/hold, live preview, and
export to `.cast` + **mp4/webm/gif** with `agg`/`ffmpeg` bundled (no prerequisites).
Agent/MCP tooling is next. See [Status](#status) and [`SPEC.md`](SPEC.md).

---

## The problem

You record a 17-minute terminal session. 90% of it is the agent (or compiler, or
deploy) *thinking* — a spinner turning, a timer incrementing, nothing to watch.
The 10% worth keeping is scattered: a command firing, a view opening, a result
landing. To ship a 40-second clip you have to find those moments and stitch them
together.

Today that means: play the cast, eyeball timestamps, run `asciinema-edit cut`
against numbers you guessed, render, discover you clipped the wrong second,
repeat. Speed-up filters flatten everything uniformly. Duplicate-frame droppers
(`mpdecimate`) nuke the readable dwell time along with the dead air.

There is no tool that **shows you the recording and lets you cut it by looking.**

## The idea

Three things existing tools don't have:

1. **An activity timeline.** Change-density per time bucket, drawn as a waveform.
   Dead air is a flat valley you can see at a glance and skip. This is the whole
   game — *knowing where to cut* is the hard part, and no CLI shows it.
2. **A filmstrip.** Real thumbnails along the timeline so you scrub against
   frames, not blind seconds.
3. **A segment model.** Non-destructive keep-ranges over the original cast, each
   with its own **speed** and **freeze/hold** — fast-forward the waiting, stay 1×
   on the payoff, hold 3s on the final frame. The manual control a "director's
   cut" actually needs.

Then: live preview, and one-click export to `.cast` + mp4/webm/gif.

## Why it's buildable

The terminal is a state machine, so you can't repaint a character mid-stream. But
to show the screen at any moment `T`, you replay the ANSI stream `0→T` through a
headless terminal emulator ([avt](https://github.com/asciinema/avt), asciinema's
own VT) and read the grid. Every edit is a **time** operation over an immutable
event list — cut, reorder, rescale, hold — recomposed into a new cast. No pixel
editing required.

## Using it

**The desktop app** (recommended — native window, native File → Open, bundled
export). Built from source today; installers come from the release workflow:

```sh
cd web
npm run tauri dev        # run the desktop app from source
npm run tauri build      # build installers for this platform
```

Open a `.cast` with **File → Open** (`Ctrl/Cmd+O`), mark segments on the timeline,
set per-segment speed/hold, preview live, then **Export** to `.cast` + `.mp4` /
`.webm` / `.gif`. `agg` and `ffmpeg` ship inside the app — nothing to install.

**Browser mode** — one binary serves the same editor as a local web app:

```sh
asciicut recording.cast           # serves the editor; prints the local URL
```

**Headless compose** — no UI, deterministic, CI-friendly:

```sh
asciicut compose recording.asciicut.json > cut.cast
```

All three run the *same* compose engine, so the output is byte-identical.

Build the core + CLI + server with `cargo build --release`; see
[`crates/asciicut-desktop/BUILD.md`](crates/asciicut-desktop/BUILD.md) for the full
per-platform packaging steps.

## Two directors: you, or an agent

The edit is a plain document — `forge_sprint.asciicut.json`: keep-ranges with per-
segment speed and hold. Whoever writes it, the compose + render pipeline is the
same. So asciicut has two front-ends to one engine:

- **You**, in the GUI — cut by looking. *(shipping today)*
- **An agent**, headless — *(planned, M6)* asciicut will expose MCP tools (`probe`,
  `frame`, `compose`, `render`) plus a Skill. An agent reads the activity signal and
  the terminal grid at any moment as **text/JSON** (not pixels), so it can find the
  dead air, read the payoff frame, and author the director's cut on its own. With a
  browser/vision tool it can also *look* — pick the hero frame and review its own
  rendered clip.

  The groundwork is already in place: `asciicut compose` composes a project
  headlessly today, and the same edit document round-trips between agent and GUI.

Agent drafts, human directs: the agent proposes a cut, you open the GUI on the same
`.asciicut.json` to refine. See [`SPEC.md` §8](SPEC.md).

## How it's built

One **Rust core** (`asciicut-core`: parse · VT · activity signal · compose) compiled
two ways — **native** for the CLI, the MCP server, and the local app's server, and
**WASM** for the zero-install web demo — so the preview, the filmstrip, the agent's
frames, and the exported cast all come from the *same* engine and can't drift apart.

| | | |
|---|---|---|
| Core / VT | Rust · [avt](https://github.com/asciinema/avt) (native + WASM) | ✅ |
| Shared ops | `asciicut-bridge` — one set of handlers behind both the server and the desktop app | ✅ |
| Frame → video | [agg](https://github.com/asciinema/agg) → ffmpeg, bundled as sidecars | ✅ |
| UI | SolidJS + Vite · [asciinema-player](https://github.com/asciinema/asciinema-player) for preview | ✅ |
| Desktop | Tauri v2 — native window, in-process core, no localhost | ✅ |
| Local server | one Rust binary = CLI + server, SPA embedded | ✅ |
| Agent | MCP via `rmcp` + a Skill | 🔲 planned |

Full reasoning and the alternative considered in [`SPEC.md` §7](SPEC.md).

## Status

| Milestone | | |
|---|---|---|
| M1 · `asciicut-core` — parse · VT · activity signal · compose | ✅ | golden-cast tests, native + WASM |
| M2 · read-only visualizer — timeline, filmstrip, player preview | ✅ | |
| M3 · segment editing — draw/drag, speed, hold, idle-cap, live preview | ✅ | |
| M4 · export — `.cast` + mp4/webm/gif | ✅ | video export ships in the desktop app |
| M7 · desktop — Tauri v2, native file dialogs, bundled sidecars | ✅ | pulled forward |
| M5 · polish — auto-suggest cuts, markers/captions, keyboard editing | 🔲 | |
| M6 · agent interface — headless CLI + MCP server + Skill | 🔲 | next |

**Platform reality:** Linux is built and verified end-to-end. macOS and Windows
installers build from the three-OS GitHub Actions matrix
(`.github/workflows/desktop-release.yml`) but have not been run on real runners
yet; a macOS build additionally needs an ffmpeg static-build URL supplied as a
secret. All S2 builds are **unsigned** — see
[`OPENING_UNSIGNED.md`](crates/asciicut-desktop/OPENING_UNSIGNED.md).

## Prior art (all CLI, none visual)

- [`cirocosta/asciinema-edit`](https://github.com/cirocosta/asciinema-edit) — cut / speed / quantize
- [`pocc/asciinema-edit`](https://github.com/pocc/asciinema-edit) — rearrange / remove sections
- [`alexyorke/asciinema-tools`](https://github.com/alexyorke/asciinema-tools) — trim / annotate
- [`asciinema-scene`](https://discourse.asciinema.org/t/editing-tool-asciinema-scene/739) — scripted edits

asciicut is the visual layer none of them have.

## Meet Nibbles

<img src="assets/logomark_head_transparent.png" align="right" width="120" alt="Nibbles the Beaver — logomark head with safety goggles" />

**Nibbles the beaver** — safety goggles on, scissors ready — is asciicut's mascot,
and turns up across the app: the header logomark, the welcome hero, the empty
states, and the export loader. The full brand kit (logos, icons, illustrations,
and a social banner) with usage guidelines lives in **[`assets/`](assets/README.md)**.

<table>
  <tr>
    <td align="center" width="33%"><img src="assets/logomark_head_transparent.png" width="110" alt="Logomark head" /><br /><sub><b>Logomark</b><br />favicon · app icon · header</sub></td>
    <td align="center" width="33%"><img src="assets/ui_empty_state_transparent.png" width="110" alt="Nibbles tangled in terminal tape" /><br /><sub><b>Empty state</b><br />nothing loaded / selected</sub></td>
    <td align="center" width="33%"><img src="assets/ui_loading_transparent.png" width="110" alt="Nibbles powering a progress bar" /><br /><sub><b>Loading</b><br />export &amp; init progress</sub></td>
  </tr>
</table>

## License

MIT © Entelligentsia
