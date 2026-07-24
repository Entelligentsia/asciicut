# asciicut — Product Spec

Status: draft v0.2 · Owner: Entelligentsia · Last updated: 2026-07-22
Tech stack locked (§7): Rust core (native + WASM) · SolidJS UI · avt · agg + ffmpeg · rmcp · Tauri v2 (later)

---

## 1. Summary

asciicut is a **visual editor for terminal session recordings** (asciinema `.cast`
files). It turns the blind, numeric CLI editing workflow into a see-what-you-cut
experience: an activity timeline that surfaces dead air, a filmstrip of real
frames, a non-destructive segment model with per-segment speed and holds, live
preview, and export to `.cast` + mp4/webm/gif.

The wedge is one insight: **finding where to cut is the hard part, and no existing
tool shows it.** asciicut makes the boring stretches visible and the good moments
draggable.

## 2. Problem & motivation

Terminal recordings of real work (AI coding agents, builds, deploys, test runs)
are mostly *waiting* — a spinner turning while a model or process thinks. A
shareable clip is a small, scattered subset of moments: a command firing, a view
opening, a result landing.

Existing editing paths all fail the "director's cut" use case:

| Approach | Failure |
|---|---|
| `asciinema-edit cut --start X --end Y` | Blind — you guess timestamps, render, discover you clipped the wrong second, repeat. |
| Global speed-up (`agg --speed`, ffmpeg `setpts`) | Flattens everything uniformly; the payoff frame flies by as fast as the dead air. |
| Duplicate-frame drop (ffmpeg `mpdecimate`) | Binary keep/drop; removes readable dwell time along with dead air (observed: a 90s cut collapsed to 3.6s). |
| Hand-editing the `.cast` JSONL | Precise but the payload is raw ANSI — practical only for cutting whole time-ranges, not surgery within a frame. |

None of these let you **look at the recording and cut by looking.** That is the
product.

## 3. Users & use cases

- **Dev-tool builders / DevRel** shipping demo clips of a CLI or TUI (the origin
  case: a coding-agent sprint recording destined for a product page).
- **Engineers** attaching a tight terminal clip to a PR, issue, or postmortem.
- **Educators / doc authors** trimming a long session into a focused walkthrough.
- **Anyone with a 15-minute cast who needs a 40-second clip.**

Primary job-to-be-done: *"I have a long terminal recording. Give me a short,
well-paced clip of the good parts, exported for the web, without wrestling ANSI or
guessing timestamps."*

## 4. Core concepts & data model

### 4.1 Source cast (immutable)
An asciinema v2/v3 `.cast`: a header (`width`, `height`, theme, …) plus an ordered
event list `[t, "o", bytes]`. asciicut never mutates the source; all edits are a
projection over it.

### 4.2 Edit project (the document)
```jsonc
{
  "source": "forge_sprint.cast",     // ref + content hash
  "output": { "width": 120, "height": 40, "theme": "forge-dark", "fontSize": 14 },
  "segments": [
    { "id": "s1", "srcStart": 25.0, "srcEnd": 31.0,  "speed": 1.0, "holdEnd": 0,   "label": "command fires" },
    { "id": "s2", "srcStart": 472.0,"srcEnd": 504.0, "speed": 2.5, "holdEnd": 1.5, "label": "dashboard opens" },
    { "id": "s3", "srcStart": 916.0,"srcEnd": 947.0, "speed": 1.0, "holdEnd": 3.0, "label": "transcript payoff" }
  ],
  "idleCap": 0.4,                      // global max gap between frames
  "markers": [ { "t": 940.0, "text": "8-phase transcript" } ]
}
```

- **Segment**: a kept `[srcStart, srcEnd]` window of the source, with its own
  `speed` multiplier and optional `holdEnd` (freeze the last frame N seconds).
- **Non-destructive**: reorder, retime, or delete segments freely; the source is
  untouched. Projects are saved as `.asciicut.json` next to the cast.

### 4.3 Compose algorithm (export)
For each segment in order: take source events in `[srcStart, srcEnd]`, rebase and
scale inter-event deltas by `1/speed`, apply `idleCap`, append; between segments
insert a beat; after a segment with `holdEnd>0` duplicate the last frame's screen
state for the hold. Emit a new `.cast` (header from `output`). This is a
generalization of the working prototype in `/prototype/compose.py`.

### 4.4 Frame-at-T
To render the screen at any source time `T`: feed events `0→T` into asciinema's
**avt** virtual terminal (§7) — applying `o` output via `feed_str`, `r` resize via
`resize`, honoring `m` markers — and read the grid via `text()` (or `view()`/
`lines()` for per-cell style). The same avt runs natively (CLI/server) and as WASM
(browser), so every surface reads an identical grid. Used for the filmstrip,
segment-boundary previews, and scrubbing.

### 4.5 Activity signal
Per fixed bucket (e.g. 0.25s): a change score = printable/cursor-affecting bytes,
optionally weighted by how much of the grid changed vs the previous bucket
(so spinner/timer-only churn scores near-zero). Rendered as the timeline waveform.
Dead air = low score = flat valley.

## 5. Features

### 5.1 MVP (v0.1)
- [ ] Open a `.cast` (drag-drop or `npx asciicut file.cast`).
- [ ] **Activity timeline** waveform with playhead + zoom/pan.
- [ ] **Filmstrip** thumbnails aligned to the timeline.
- [ ] **Segment track**: draw/drag/resize keep-ranges; delete; reorder.
- [ ] Per-segment **speed** and **holdEnd** controls.
- [ ] Global **idle cap** slider.
- [ ] **Live preview** of the composed edit (asciinema-player).
- [ ] **Export** composed `.cast`; emit the `agg`/`ffmpeg` command for video.
- [ ] Save/load `.asciicut.json` project.

### 5.2 v1
- [ ] One-click video export (mp4/webm/gif) via bundled agg + ffmpeg.
- [ ] Markers/annotations → optional caption track burned into video.
- [ ] Auto-suggest cuts: propose segment boundaries from the activity signal
      (flag valleys > N seconds as "dead air — cut?").
- [ ] Theme/font/dimension controls with live preview.
- [ ] Keyboard-first editing (in/out marks, ripple delete, nudge).

### 5.3 Later
- [ ] Trim *within* a frame's dwell (retime holds), speed ramps.
- [ ] Multi-cast stitching (compose clips from several recordings).
- [ ] Redaction: blur/replace regions by text match (secrets, paths) across frames.
- [ ] Web SaaS: upload, edit, share a hosted player link.
- [ ] asciinema v3 format nuances (markers, resize events) first-class.

## 6. Non-goals (at least for v1)
- Not a recorder — you bring the `.cast` (record with asciinema/agg as usual).
- Not a per-pixel/per-character frame painter (the state-machine constraint).
- Not a general video editor — scope is terminal recordings.

## 7. Architecture & tech stack (locked)

**Decision: a single Rust core, compiled to native (CLI · MCP · local server) and
to WASM (static web demo), with a thin SolidJS web UI.** This is locked, not
proposed. The reasoning is §7.1, the shape §7.2, the per-layer choices §7.3, and
the option it was chosen against — stated honestly — is §7.5.

### 7.1 The forcing constraint
asciicut must produce **byte-identical** results across three surfaces: the GUI
preview, the CLI/agent, and the exported `.cast`. If the browser's virtual
terminal and the headless one disagree on a single escape sequence, the filmstrip,
the agent's `frame` text (§8), and the composed output diverge — and "look at the
recording and cut by looking" (§1) quietly breaks. That demands **one VT and one
compose engine, shared by every surface**, not two implementations kept in sync by
hope.

The asciinema ecosystem asciicut sits on is Rust, and — decisively — asciinema's own
VT already runs in the browser:

- **avt** (asciinema virtual terminal, Rust) is the emulator behind the asciinema
  CLI, player, and agg. It exposes the screen grid as text (`text()`) and cells
  (`view()`/`lines()`), and **compiles to WASM in production** — the asciinema
  player ships it (Rust VT → WASM; "4× smaller, 50× faster" than the old JS VT).
- **agg** (the proven renderer, §9) is Rust, built on avt, and usable as a
  **library** (`agg::Renderer`) — so frame→PNG for the filmstrip and the agent's
  `frame --png` reuses agg's font/emoji/theme rasterizer instead of a
  reimplementation.

So one Rust core reuses avt + agg and runs *unchanged* natively and in the browser.
No other language delivers byte-identical parity **and** a browser VT without
maintaining a second emulator (e.g. xterm.js), whose output is not byte-identical
to avt.

### 7.2 Shape

```
                        ┌──────────────────────────────────┐
                        │  asciicut-core   (Rust crate)      │
                        │  • .cast v2/v3 parse (delta time) │
                        │  • avt VT → frame-at-T (grid,§4.4)│
                        │  • activity signal (§4.5)         │
                        │  • compose: windows/speed/        │
                        │    idle-cap/holds (§4.3)          │
                        │  • agg::Renderer → frame PNG      │
                        └──────────────────────────────────┘
             native ↑            native ↑              ↑ wasm32-unknown
      ┌─────────────┘                   │              └──────────────┐
┌─────────────────┐        ┌────────────────────┐        ┌────────────────────────┐
│ asciicut  (CLI)  │        │ asciicut-mcp        │        │ asciicut_core.wasm      │
│ probe · frame · │        │ (rmcp: stdio +     │        │ static web demo — VT + │
│ compose · render│        │  streamable HTTP)  │        │ compose in-browser,    │
└─────────────────┘        └────────────────────┘        │ no server (§7.4)       │
      │ shells agg(lib)+ffmpeg for video                 └────────────────────────┘
      │                                                                ▲
      └──────────► asciicut-server (axum + rust-embed) ─────────────────┤
                     • `npx asciicut file.cast` launches it             │ imports
                     • serves the SPA; reads/writes .cast/.asciicut.json│ core.wasm
                     • /frame /thumbs /compose /render  (native, fast) │ (static
                                     │                                 │  build only)
                                     ▼                                 │
                        ┌──────────────────────────────────┐          │
                        │  Web SPA  (SolidJS + Vite)        │──────────┘
                        │  • timeline · filmstrip · segment │
                        │    track  (canvas)                │
                        │  • asciinema-player preview, fed  │
                        │    in-memory (create({data:…}))   │
                        │  • window.asciicut command API(§8.4)│
                        └──────────────────────────────────┘
```

### 7.3 Locked stack, by layer

| Layer | Locked choice | Product reason |
|---|---|---|
| Core language | **Rust** (one `asciicut-core` crate) | one VT + compose, byte-identical on native and WASM; reuses avt + agg |
| VT / frame-at-T | **`avt`** crate | asciinema's own emulator; grid as text and cells; WASM-proven in the player |
| Compose engine | **Rust** (port `/prototype/compose.py`) | the shared contract of §4.2/§4.3; golden-cast tests; runs everywhere |
| Frame → PNG | **`agg` as a library** (`agg::Renderer`) | reuse agg's raster for filmstrip + `frame --png`; no font stack to own |
| Video export | **`agg` → `ffmpeg`** (unchanged, §9) | most deterministic, headless, CI-friendly `.cast`→video path; two static deps |
| Web preview | **asciinema-player v3**, fed **in-memory** (`create({ data })`) + `seek()` / `poster:'npt:…'` | official, ~140 KB, WASM VT makes scrubbing instant; accepts in-memory casts |
| Web UI | **SolidJS** (+ Vite, `vite-plugin-wasm`) | fine-grained signals suit a canvas-redraw editor; same framework as the player |
| Local app | **one Rust binary = CLI + axum server**, SPA baked in via **`rust-embed`** | `npx asciicut file.cast` serves the GUI and does compute natively; one artifact |
| Agent / MCP | **`rmcp`** (official Rust SDK), `#[tool]` macros, stdio + HTTP | wraps core functions **in-process** — no shelling a CLI per tool call |
| Distribution | **`cargo-dist`** → brew · `curl\|sh` · **npx wrapper** · `binstall` | keeps the `npx asciicut` one-liner *and* single-binary installs from one config |
| Desktop (later) | **Tauri v2** | Rust-native (reuses core); ~3–5 MB vs Electron ~100 MB; sidecar-bundles agg/ffmpeg |

### 7.4 One engine, two compile targets
The WASM-vs-server question is not a fork to resolve — it's the same core, and the
surface decides who runs it:
- **`npx asciicut` local app** → the browser is a thin client; the native Rust
  server does frame / thumbnail / compose / render (fast, and agg renders PNGs
  directly on disk).
- **zero-install static web demo** → `asciicut-core` compiled to **WASM** does
  frame-at-T + compose in the browser, no server, no filesystem.

Write the engine once; ship it to both.

### 7.5 Alternative considered — Node/TypeScript (and when it would win)
A Node/TS core (Vite + TS SPA, `@xterm/headless` for the VT, a Node sidecar) is the
faster start and has real advantages, stated plainly:
- The **MCP TypeScript SDK is Tier 1**; Rust's `rmcp` is Tier 2 (official and
  capable, but more API churn, fewer examples).
- `npx` is **native** to Node; faster UI iteration; larger talent pool.

It was **rejected on the forcing constraint (§7.1)**: it needs *two* virtual
terminals — xterm.js in the browser, something else headless — whose output is not
byte-identical, so preview, thumbnails, agent `frame` text, and export can diverge.
Its desktop path is also heavier (Electron's ~100 MB, or Tauri carrying a bundled
Node runtime — reintroducing a second runtime exactly where we wanted fewer moving
parts).

**When Node would be the right call instead:** a JS-first team optimizing for a
web+CLI MVP shipped fast, with desktop and cross-surface byte-parity treated as
non-goals. asciicut's north star is the opposite (§1, §4.2, §5.3), so Rust wins here.

### 7.6 Grounding
avt→WASM in the player: blog.asciinema.org/post/smaller-faster · in-memory player
API: docs.asciinema.org/manual/player/loading · agg as a Rust lib on avt:
github.com/asciinema/agg · MCP SDK tiers / rmcp: modelcontextprotocol.io/docs/sdk ·
Tauri v2 sidecar + size: buildmvpfast.com/blog/tauri-v2-vs-electron-desktop-apps-2026.

## 8. Agent interface (headless + visual)

asciicut is designed for two directors: a **human** in the GUI, and an **agent**
producing a director's cut autonomously. Both are first-class, and they cost
almost nothing extra to support because of one property of the data model:

> The edit project `.asciicut.json` (§4.2) is a **declarative document**, not a
> replay of GUI actions. Whoever authors it — human or agent — emits the same
> artifact, and the compose (§4.3) + render (§9) pipeline is agnostic to the
> author. The GUI and the agent are two front-ends to one back-end.

And asciicut's core primitives are already **agent-native**: the activity signal
(§4.5) is a change-density array and frame-at-T (§4.4) is a terminal grid as text —
JSON and text, not pixels. An agent can reason over them directly.

```
                    ┌── human, in the GUI ────┐
raw .cast ──────────┤                         ├─▶ .asciicut.json ─▶ compose ─▶ agg/ffmpeg ─▶ clip
                    └── agent, headless/visual ┘        (the seam)
```

### 8.1 Tool surface (headless CLI = MCP tools)
The agent path needs a headless CLI; the same commands are wrapped as MCP tools so
any agent (e.g. Claude Code) can call them. `compose` exists in the prototype; the
rest are the M6 build target (§12).

| Tool | Returns | Role |
|---|---|---|
| `asciicut probe <cast> --json` | duration, dims, theme, activity buckets, detected **dead-air valleys** (>Ns), candidate **activity peaks** | the agent's eyes on the *timeline* |
| `asciicut frame <cast> --at <T> [--format text\|json\|png]` | the screen at time `T` — text grid, cell JSON, or a rendered PNG | the agent's eyes on a *frame* (kills blind-timestamp guessing) |
| `asciicut compose <project.json>` | composed `.cast` | ports `/prototype/compose.py` |
| `asciicut render <project.json> --format mp4\|webm\|gif [--screenshot <t>]` | rendered clip (+ optional still) | the proven agg/ffmpeg recipe (§9) |
| `asciicut suggest <cast>` | heuristic first-pass `.asciicut.json` | non-agent baseline of §5.2 auto-suggest |

### 8.2 Two agent modes
**Mode A — headless (text/JSON).** The default authoring loop, deterministic and
cheap:

```
probe ─▶ for each peak: frame(T, text) to read/verify content
      ─▶ decide keep / speed / hold / label
      ─▶ assemble segments ─▶ write .asciicut.json ─▶ compose + render
```

**Mode B — visual.** With a browser/vision tool (Puppeteer, Playwright, or an
in-browser agent) the agent also gets eyes. Two sub-modes with different jobs:

- **Images as tool output** (cheap, deterministic): `frame --format png`, filmstrip
  PNGs, or `render --screenshot`. The agent *looks* at footage without driving a
  UI. Best for judgment — see §8.3.
- **Driving the GUI** (human-like, but pixel-clicking is brittle): the agent
  operates the same interface a human does. To keep it semantic rather than
  coordinate-fragile, the GUI exposes an automation-first command surface (§8.4);
  pixel interaction is the fallback, not the plan.

### 8.3 Where vision earns its cost
Vision is a **judgment / self-critique layer over text-first authoring**, not the
authoring interface. Spend it only where a text grid is blind — color, TUI layout
integrity, aesthetic pacing:

| Job | Interface | Why |
|---|---|---|
| Find dead air / candidates | `probe` (json) | change-density array; no image needed |
| Pick cut points | `frame --text` | reads grid content deterministically |
| **Choose the hero shot** | `frame --png` (vision) | which frame *looks* strong — color, density, layout |
| **Verify the rendered clip** | `render --screenshot` (vision) | theme correct? box-drawing aligned? payoff legible? pacing right? |

The load-bearing move is the last row: after rendering, the agent **reviews its own
output with eyes** — the same self-critique a human does in the preview pane — and
iterates the `.asciicut.json`. The full loop:

```
probe → frame-text (points) → frame-png (hero shots) → write .asciicut.json
      → render → screenshot-critique → adjust → repeat
```

### 8.4 GUI command surface (for Mode B and tests)
So an agent drives the GUI by semantics, not coordinates, the SPA exposes an
imperative API on `window.asciicut` plus `data-testid` hooks. The same surface is
the GUI's own end-to-end test harness, so it pays for itself.

```ts
window.asciicut.open('forge_sprint.cast')
window.asciicut.addSegment({ srcStart: 916, srcEnd: 947, speed: 1, hold: 3, label: 'payoff' })
window.asciicut.select('s2'); window.asciicut.setSpeed(2); window.asciicut.setHold(1.5)
window.asciicut.setIdleCap(0.4)
const project = window.asciicut.getProject()      // → the .asciicut.json in memory
await window.asciicut.export({ format: 'mp4' })    // writes .asciicut.json + renders
```

### 8.5 The Skill
`SKILL.md` encodes the **director's judgment** as instructions the agent follows —
the generalization of §5.2 from "suggest boundaries" to "author the whole cut with
reasoning over frame content":

> Valleys > ~20s are dead air → cut. Payoff frames stay 1× with a 2–3s end-hold so
> viewers can read. Compress drawing/scrolling at 2×+. Target 30–60s. Label each
> segment from what `frame` shows. Render, then critique the still before shipping.

This enables an **agent-drafts, human-directs** loop: the agent emits
`forge_sprint.asciicut.json`, the human opens the GUI on the same document to refine.

## 9. Export pipeline (proven)
The video path already works end-to-end and is the reference for v1:
```
compose → edited.cast
agg  --theme <t> --font-family <f> --font-size <n> --fps-cap 24 edited.cast edited.gif
ffmpeg -i edited.gif -pix_fmt yuv420p -movflags +faststart edited.mp4
ffmpeg -i edited.gif -c:v libvpx-vp9 -b:v 0 -crf 34 -an   edited.webm
```
Holds and per-segment speed are baked into `edited.cast` by the compose step, so
the render stage stays a dumb, deterministic transform.

## 10. Success criteria
- A 17-min raw cast → a shipped 40s clip in **under 5 minutes**, without opening a
  text editor or guessing a timestamp.
- The activity timeline makes dead air obvious at a glance (user can cut it
  without playing the whole recording).
- Exported clip preserves crisp text, correct colors, readable dwell on payoff
  frames.
- Round-trips: `.asciicut.json` reopens to the exact edit state.

## 11. Open questions

Resolved by §7 (tech stack locked):
- ~~UI framework~~ → **SolidJS** (§7.3).
- ~~Distribution~~ → **`npx` local app first** (one Rust binary), static web demo
  as a WASM build target, **Tauri v2** desktop later (§7.3–7.4).
- ~~v3 format~~ → **support asciinema v3 natively**; avt handles v2/v3 and the
  parser accumulates delta timing, applying `r`/`m` events on replay (§4.4).

Still open:
1. **Name** — `asciicut` provisional. Alternatives: cutting-room, reel, clapper.
2. **Activity metric** — bytes-based (cheap) vs grid-diff (accurate, needs a VT per
   bucket). Likely: cheap first pass, VT-diff refine on zoom. (avt makes the
   VT-diff path affordable natively.)
3. **Open-source vs product** — MIT alongside grove/forge, or hosted SaaS tier for
   the web-share path?

## 12. Milestones
- **M0 — spec + repo** (this document). ✅
- **M1 — `asciicut-core` (Rust)**: `.cast` v2/v3 parse, avt frame-at-T, activity
  signal, and compose (port of `/prototype/compose.py`), golden-cast tests, plus
  the `asciicut compose` CLI. The one engine every later surface reuses (§7).
- **M2 — read-only visualizer**: `asciicut-server` (axum + rust-embed) + SolidJS SPA
  → load cast → activity timeline + filmstrip + asciinema-player preview. Proves
  frame-at-T and the activity signal end to end (native path; WASM build as a
  stretch).
- **M3 — segment editing**: draw/drag segments, per-segment speed/hold, live
  preview of the composed edit fed to the player in-memory. Wire `window.asciicut`
  (§8.4) here so the GUI is scriptable/testable from day one.
- **M4 — export**: `.cast` + one-click mp4/webm/gif via `agg` (as a library) +
  `ffmpeg` (§9), invoked by the server.
- **M5 — polish**: auto-suggest cuts, markers/captions, keyboard editing.
- **M6 — agent interface** (§8): headless `probe`/`frame`/`render` CLI + an **`rmcp`
  MCP server** (stdio + streamable HTTP) wrapping `asciicut-core` in-process, and
  `SKILL.md`. Adds `frame --png` (via `agg::Renderer`) + `render --screenshot` for
  the visual self-critique loop. Reuses M1 compose and the M2 frame/activity engine.
- **M7 — desktop (optional)**: **Tauri v2** app reusing `asciicut-core`, sidecar-
  bundling `agg`/`ffmpeg` so there are no external prerequisites.

## 13. Prototype assets already in hand
- Working event-level compose script (windows + idle-cap + holds) — port target
  for M1.
- Proven `agg`+`ffmpeg` render recipe (§9).
- A real 17-min test cast (coding-agent sprint) with known dead-air structure —
  the canonical fixture for the activity timeline and the "5-minute clip" test.
