# asciicut — Claude Code Instructions

## Project Identity

**asciicut** is a visual editor for asciinema `.cast` terminal recordings. It turns
blind CLI editing into a see-what-you-cut experience: an activity timeline that
surfaces dead air, a filmstrip of real frames, a non-destructive segment model
with per-segment speed and holds, live preview, and export to `.cast` + mp4/webm/gif.

**Status:** working. The Rust core, `axum` server, SolidJS SPA, and a native
Tauri desktop app ship today: activity timeline, filmstrip, segment editing with
per-segment speed/hold, live preview, and export to `.cast` + mp4/webm/gif with
`agg`/`ffmpeg` bundled. The `prototype/` dir keeps the original Python compose
reference and the static UI mock for provenance. Agent/MCP tooling is next.

**License:** MIT © Entelligentsia

---

## Tech Stack (locked — SPEC §7)

| Layer | Choice | Notes |
|-------|--------|-------|
| Core language | **Rust** — single `asciicut-core` crate | Compiles native (CLI/server) + WASM (browser). One VT, one compose engine, byte-identical everywhere. |
| VT / frame-at-T | **`avt`** crate | asciinema's own emulator; WASM-proven in the player. |
| Frame → PNG | **`agg`** as a library (`agg::Renderer`) | Reuses agg's font/emoji/theme rasterizer. |
| Video export | **`agg` → `ffmpeg`** | Deterministic `.cast` → video path. |
| Web preview | **asciinema-player v3** | Fed in-memory (`create({ data })` + `seek()`). |
| Web UI | **SolidJS** + Vite + `vite-plugin-wasm` | Fine-grained signals for canvas-redraw editor. |
| Local server | **axum** + `rust-embed` | One binary = CLI + server; SPA baked in. |
| Agent / MCP | **`rmcp`** (Rust MCP SDK) | `#[tool]` macros, stdio + HTTP; wraps core in-process. |
| Desktop (later) | **Tauri v2** | Reuses core; sidecar-bundles agg/ffmpeg. |
| Distribution | **`cargo-dist`** → brew · `curl\|sh` · npx wrapper · `binstall` | |

**Prototype-only (not production):** Python 3 (`prototype/compose.py` — reference
compose engine to be ported to Rust in M1).

### Commands

```sh
cargo build --release          # build
cargo test                     # test (golden-cast tests for compose)
cargo clippy --all-targets --all-features -- -D warnings   # lint
```

Run `cargo` commands from the repo root; the SolidJS SPA lives in `web/` (`npm
install` then `npm run dev` / `npm run build`). The desktop shell is
`crates/asciicut-desktop` (built standalone — it is excluded from the workspace).

---

## Architecture

```
asciicut-core (Rust crate)
  ├── .cast v2/v3 parse (delta time)
  ├── avt VT → frame-at-T (grid as text/cells)
  ├── activity signal (change-density per time bucket)
  ├── compose: windows/speed/idle-cap/holds
  └── agg::Renderer → frame PNG

     native ↑                    ↑ wasm32-unknown
┌─────────────┘                  └──────────────┐
asciicut CLI    asciicut-mcp (rmcp)    asciicut_core.wasm
  · probe        · stdio + HTTP        (static web demo)
  · frame
  · compose
  · render
       │
       └──► asciicut-server (axum + rust-embed)
               · npx asciicut file.cast → serves SPA
               · /frame /thumbs /compose /render (native)
                        │
                        ▼
               Web SPA (SolidJS + Vite)
               · timeline · filmstrip · segment track
               · asciinema-player preview (in-memory)
               · window.asciicut command API
```

### Data model (file-based, no database)

- **Source Cast** (`.cast`) — immutable. JSON header + newline-delimited `[t, code, data]` events.
- **Edit Project** (`.asciicut.json`) — declarative edit document: source ref, segments, markers, output settings, idle cap.
- **Segment** — `[srcStart, srcEnd]` window with `speed` and `holdEnd`.
- **Compose is deterministic and surface-agnostic** — same VT + compose engine on all surfaces (SPEC §7.1).

---

## Project Conventions

### File layout

```
asciicut/
├── crates/
│   ├── asciicut-core/    # .cast parse, avt frame-at-T, activity signal, compose (native + WASM)
│   ├── asciicut/         # CLI (probe · frame · compose · render)
│   ├── asciicut-server/  # axum server + rust-embed SPA
│   ├── asciicut-bridge/  # shared DTOs / path derivation across server + desktop
│   └── asciicut-desktop/ # Tauri v2 shell (standalone; excluded from the workspace)
├── web/                  # SolidJS + Vite SPA (timeline, filmstrip, segment track, player)
├── assets/               # Nibbles brand assets (logos, mascot, illustrations)
├── prototype/            # Original Python compose reference + static HTML/CSS UI mock
├── samples/              # Test fixtures (sample.cast + sample.asciicut.json)
├── .github/              # CI + release workflows
├── SPEC.md               # Full product spec (canonical)
└── README.md             # Project overview
```

### Naming & style

- **Rust:** standard Rust conventions (`snake_case`, `CamelCase` types). `cargo fmt` + `cargo clippy -- -D warnings`.
- **TypeScript/SolidJS:** standard TS conventions. Prettier + ESLint (config TBD once scaffolded).
- **Commits:** conventional commits preferred (`feat:`, `fix:`, `docs:`, `chore:`).
- **Branching:** feature branches from `main`; PRs required for merge.

### Key design rules

1. **The source `.cast` is immutable.** Never mutate it. All edits are projections.
2. **One VT, one compose engine.** `asciicut-core` is the single source of truth for frame-at-T and compose. No second implementation.
3. **Byte-identical output across surfaces.** GUI preview, CLI compose, and export must produce identical `.cast` output.
4. **Non-destructive editing.** Segments are kept windows; reorder/retime/delete freely without touching the source.
5. **`idleCap` is global.** Applied during compose so no dead-air gap exceeds the cap.

---

## Milestones (from SPEC §12)

| Milestone | Status | Description |
|-----------|--------|-------------|
| M0 — spec + repo | ✅ | SPEC.md, README, prototype, samples |
| M1 — asciicut-core | ✅ | .cast parse, avt frame-at-T, activity signal, compose, golden-cast tests, `asciicut compose` CLI |
| M2 — read-only visualizer | ✅ | axum server + SolidJS SPA: load cast, timeline, filmstrip, player preview |
| M3 — segment editing | ✅ | Draw/drag segments, speed/hold, idle-cap, live preview |
| M4 — export | ✅ | `.cast` + mp4/webm/gif via bundled agg + ffmpeg |
| M7 — desktop | ✅ | Tauri v2 app, native dialogs, bundled sidecars |
| M5 — polish | 🔲 | Auto-suggest cuts, markers/captions, keyboard editing |
| M6 — agent interface | 🔲 | Headless probe/frame/render CLI + rmcp MCP server + SKILL.md |

---

## Quick Reference

- **Canonical spec:** `SPEC.md` — read this first for any design question.
- **Sample data:** `samples/sample.cast` + `samples/sample.asciicut.json`.
- **Prototype compose:** `prototype/compose.py` — the original reference engine the Rust core was ported from.
- **Brand assets:** `assets/` — Nibbles the beaver (see `assets/README.md`).

> Sprint/task planning and the engineering knowledge base are maintained in a
> separate private repository (`asciicut-engineering`) that wraps this one, and
> are intentionally not part of the public product tree.
