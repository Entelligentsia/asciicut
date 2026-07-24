// Typed client for the asciicut command surface — asciicut-server's `/api/*`
// HTTP routes (web-demo) or asciicut-desktop's Tauri commands (native shell),
// both thin transports over the shared asciicut-bridge crate — plus TypeScript
// mirrors of its wire DTOs.
//
// ─── DTO PARITY CONTRACT ─────────────────────────────────────────────────────
// These interfaces MANUALLY MIRROR `crates/asciicut-bridge/src/dto.rs` (moved
// there from asciicut-server in CASTCU-S2-T02 — both the axum `/api/*` surface
// and the Tauri command surface now serialize that exact same Rust
// `Serialize` impl, so this file only needs to track ONE Rust source of
// truth, not two). There is no codegen seam yet, so Rust `serde` output and
// these types can drift — that is the key coupling risk for this file. Rules
// reproduced from the Rust side:
//   • StyleDto boolean attrs use `#[serde(skip_serializing_if = Not::not)]` —
//     a `false` attr is OMITTED from the JSON, so every bool is OPTIONAL here.
//   • foreground/background use `skip_serializing_if = Option::is_none` — absent
//     when unset, so both are OPTIONAL.
//   • ColorDto is `#[serde(tag = "kind", rename_all = "lowercase")]` — a tagged
//     union with `kind: "indexed" | "rgb"`.
//   • ActivitySignalDto is a plain struct { bucket_secs: number; buckets: number[] }
//     — no skip attrs, so both fields are always present.
// A future OpenAPI/codegen step is the durable fix (see PLAN Data Model Changes).
// ─────────────────────────────────────────────────────────────────────────────

import { invoke, isTauri } from "@tauri-apps/api/core";

import type { ProjectDto } from "./segments";

// ─── TRANSPORT SEAM ──────────────────────────────────────────────────────────
// Two transports carry the same six DTO-shaped calls below: `fetch` against
// `asciicut-server`'s `/api/*` (web-demo, and today's desktop dev server) or
// Tauri `invoke()` against the in-process command bridge (`crates/
// asciicut-desktop/src/commands.rs`, CASTCU-S2-T02). `isTauri()` — from
// `@tauri-apps/api/core`, not a build-time env flag — is checked at CALL TIME,
// so the exact same bundle works unmodified in the browser and inside the
// native webview; there is no per-call-site branch to hand-edit and no build
// mode that bakes the choice in. `exportProject` is explicitly NOT part of
// this seam — see its own doc comment below.
//
// Command names below (`"frame"`, `"thumbs"`, `"activity"`, `"compose_project"`,
// `"save_project"`, `"project_meta"`) and their argument object keys are the
// literal Rust fn names / parameter names `commands.rs` registers — Tauri's
// `invoke()` dispatches on that exact string, with no camelCase rewriting for
// these single-word argument names.
// ─────────────────────────────────────────────────────────────────────────────

/** A terminal color — tagged so palette and true-color are distinguishable. */
export type ColorDto =
  | { kind: "indexed"; index: number }
  | { kind: "rgb"; r: number; g: number; b: number };

/** Foreground/background color plus boolean text attributes of a cell. */
export interface StyleDto {
  foreground?: ColorDto;
  background?: ColorDto;
  bold?: boolean;
  faint?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  blink?: boolean;
  inverse?: boolean;
}

/** A single styled grid cell. `width` is 1 (normal) or 2 (wide-char head). */
export interface CellDto {
  ch: string;
  width: number;
  style: StyleDto;
}

/** Cursor position (0-based) and visibility. */
export interface CursorDto {
  col: number;
  row: number;
  visible: boolean;
}

/** A styled terminal screen snapshot at one source time `T`. */
export interface FrameDto {
  width: number;
  height: number;
  text: string[];
  cells: CellDto[][];
  cursor: CursorDto;
  marker?: string;
}

/** One filmstrip sample: the source time and the frame at that time. */
export interface ThumbDto {
  t: number;
  frame: FrameDto;
}

/**
 * The change-density waveform of the launch cast: the bucket duration plus the
 * ordered per-bucket score array. `duration ≈ buckets.length * bucket_secs`.
 */
export interface ActivitySignalDto {
  bucket_secs: number;
  buckets: number[];
}

/** Response from `POST /api/save`: the server-derived path and byte count. */
export interface SaveResponse {
  path: string;
  bytes: number;
}

/**
 * Response from `GET /api/project`: the launch cast's authoritative source
 * filename plus any persisted `.asciicut.json` (echoed as a validated project, or
 * `null` when none is on disk yet). Mirrors the server `ProjectMeta` shape.
 */
export interface ProjectMeta {
  /** The launch cast's `file_name` — the authoritative project `source`. */
  source: string;
  /** The persisted project, or `null` when no `.asciicut.json` exists yet. */
  project: ProjectDto | null;
}

/**
 * The payload every desktop "a cast is now loaded" path produces — the result
 * of `open_cast` / `open_cast_path` / `open_sample` AND the body of the
 * `cast-opened` event. Mirrors the Rust `commands::OpenedCast`.
 *
 * It is a {@link ProjectMeta} plus the resolved absolute `path`. The path
 * travels with every open because that is what the welcome screen's **Recent**
 * list needs to re-open a recording later — including one chosen through the
 * native picker, whose path the SPA otherwise never sees.
 */
export interface OpenedCast {
  /** Absolute path the cast was loaded from. */
  path: string;
  /** The authoritative source name plus any persisted sibling project. */
  meta: ProjectMeta;
}

/**
 * Response from `POST /api/export`: the two server-derived paths written (the
 * `.asciicut.json` project and the composed `.cast`) plus the composed byte count.
 */
export interface ExportResponse {
  /** Path the `.asciicut.json` project was written to. */
  projectPath: string;
  /** Path the composed `.cast` was written to. */
  castPath: string;
  /** Byte length of the composed `.cast` document. */
  bytes: number;
}

/**
 * Desktop-only video export format choice — mirrors
 * `asciicut_desktop_lib::export::ExportFormat`'s SPA-facing string encoding
 * (CASTCU-S2-T05).
 */
export type ExportVideoFormat = "mp4" | "webm" | "gif";

/**
 * Response from the Tauri `export_video` command: the round-trip triple's two
 * paths (project + composed cast, same shape as {@link ExportResponse}) plus
 * the rendered video's own path. `bytes` mirrors the composed `.cast`
 * document's byte length (not the video file), matching
 * `ExportResponse.bytes`'s semantics.
 */
export interface ExportVideoResult {
  projectPath: string;
  castPath: string;
  videoPath: string;
  bytes: number;
}

/**
 * Stage markers carried by the `export-progress` Tauri event
 * (`asciicut_desktop_lib::export::ExportStage`, kebab-case on the wire).
 */
export type ExportStage =
  | "writing-project"
  | "writing-cast"
  | "agg"
  | "ffmpeg"
  | "done"
  | "error"
  | "cancelled";

/**
 * Payload for the `export-progress` Tauri event, streamed throughout
 * `export_video`. `percent` is only ever set during the `ffmpeg` stage's
 * fine-grained progress (`agg` has no machine-readable progress output —
 * an honest limitation, not a bug); `message` is set on `error`; `path` is
 * set on `done`.
 */
export interface ExportProgressPayload {
  stage: ExportStage;
  percent?: number;
  message?: string;
  path?: string;
}

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`${url} → ${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

/**
 * POST a `.asciicut.json` project body and return the raw response text. The
 * error path carries the server's own message (the bridge's actionable "invalid
 * project: …" / "cannot write …: …" diagnostic), not just a status code.
 */
async function postProject(url: string, projectJson: string): Promise<string> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: projectJson,
  });
  if (!res.ok) {
    throw new Error(`${url} → ${res.status}: ${await res.text()}`);
  }
  return res.text();
}

/** {@link postProject} for the endpoints that answer with a JSON document. */
async function postProjectForJson<T>(
  url: string,
  projectJson: string,
): Promise<T> {
  return JSON.parse(await postProject(url, projectJson)) as T;
}

/**
 * `GET /api/frame?t=<seconds>` (fetch) / `frame` command (Tauri) — the styled
 * screen grid at source time `t`.
 */
export function fetchFrame(t: number): Promise<FrameDto> {
  if (isTauri()) {
    return invoke<FrameDto>("frame", { t });
  }
  return getJson<FrameDto>(`/api/frame?t=${encodeURIComponent(t)}`);
}

/**
 * `GET /api/thumbs?count=<n>` (fetch) / `thumbs` command (Tauri) — `n`
 * evenly-spaced filmstrip frames.
 */
export function fetchThumbs(count?: number): Promise<ThumbDto[]> {
  if (isTauri()) {
    return invoke<ThumbDto[]>("thumbs", { count: count ?? null });
  }
  const q = count === undefined ? "" : `?count=${encodeURIComponent(count)}`;
  return getJson<ThumbDto[]>(`/api/thumbs${q}`);
}

/**
 * `GET /api/activity?bucket=<seconds>` (fetch) / `activity` command (Tauri) —
 * the change-density waveform. Both transports clamp a finite-positive
 * `bucket` up to the same `MIN_BUCKET_SECS` floor (the bridge op both share),
 * so a tiny value cannot crash either; the response echoes the effective
 * `bucket_secs`.
 */
export function fetchActivity(bucket?: number): Promise<ActivitySignalDto> {
  if (isTauri()) {
    return invoke<ActivitySignalDto>("activity", { bucket: bucket ?? null });
  }
  const q = bucket === undefined ? "" : `?bucket=${encodeURIComponent(bucket)}`;
  return getJson<ActivitySignalDto>(`/api/activity${q}`);
}

/**
 * `GET /api/events` (fetch) / `event_times` command (Tauri) — the loaded cast's
 * real event timestamps, ascending and de-duplicated.
 *
 * The editor snaps IN/OUT nudges to these (`lib/events.ts`), so a nudged
 * boundary always lands on a frame where something actually changed. Both
 * transports serve `asciicut_bridge::ops::event_times`, so the grid is the same
 * on desktop and in the browser.
 */
export function fetchEventTimes(): Promise<number[]> {
  if (isTauri()) {
    return invoke<number[]>("event_times");
  }
  return getJson<number[]>("/api/events");
}

/**
 * `POST /api/compose` (fetch) / `compose_project` command (Tauri) — compose an
 * edit project against the launch cast. Returns the composed `.cast` document
 * as text (JSON Lines, not a JSON value).
 */
export async function composeProject(projectJson: string): Promise<string> {
  if (isTauri()) {
    return invoke<string>("compose_project", { project: projectJson });
  }
  return postProject("/api/compose", projectJson);
}

/**
 * `POST /api/save` (fetch) / `save_project` command (Tauri) — persist an edit
 * project to the server/session-derived path. The client never sends a path
 * (path-traversal surface lives server/bridge-side).
 */
export async function saveProject(projectJson: string): Promise<SaveResponse> {
  if (isTauri()) {
    return invoke<SaveResponse>("save_project", { project: projectJson });
  }
  return postProjectForJson<SaveResponse>("/api/save", projectJson);
}

/**
 * Tauri-only `save_project_as` command — show the native save dialog, write the
 * project to the chosen location, and make that location the new active save
 * path. Cancellation returns `null` rather than throwing.
 */
export async function saveProjectAs(
  projectJson: string,
): Promise<SaveResponse | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<SaveResponse>("save_project_as", { project: projectJson });
  } catch (err) {
    if (err instanceof Error && err.message === "cancelled") return null;
    throw err;
  }
}

/**
 * Tauri-only `open_cast` command — show the native `.cast` file picker and load
 * the chosen recording. The shell emits a `cast-opened` event with the same
 * payload; this wrapper returns it directly for callers that prefer a
 * promise. Cancellation is a silent no-op (`null`).
 */
export async function openCast(): Promise<OpenedCast | null> {
  if (!isTauri()) {
    throw new Error("openCast is only available in the Tauri desktop shell");
  }
  try {
    return await invoke<OpenedCast>("open_cast");
  } catch (err) {
    if (err instanceof Error && err.message === "cancelled") return null;
    throw err;
  }
}

/**
 * Tauri-only `open_cast_path` command — load a recording from an explicit path
 * with no dialog. Backs the welcome screen's **Recent** list, which only ever
 * replays a path a previous open already produced (see the Rust command's own
 * provenance note).
 *
 * A moved or deleted file rejects with the bridge's diagnostic rather than
 * `"cancelled"`, which is what lets the welcome screen prune the dead entry.
 */
export async function openCastPath(path: string): Promise<OpenedCast> {
  if (!isTauri()) {
    throw new Error("openCastPath is only available in the Tauri desktop shell");
  }
  return invoke<OpenedCast>("open_cast_path", { path });
}

/**
 * Tauri-only `open_sample` command — load the bundled `sample.cast` (the
 * welcome screen's "Try the sample"). Resolves to the same {@link OpenedCast}
 * every other open path produces, so the sample lands in Recent like any other
 * recording.
 */
export async function openSample(): Promise<OpenedCast> {
  if (!isTauri()) {
    throw new Error("openSample is only available in the Tauri desktop shell");
  }
  return invoke<OpenedCast>("open_sample");
}

/**
 * Tauri-only `cast_path` command — the absolute path of the currently loaded
 * cast, or `null` when the shell launched with no recording. The startup
 * counterpart of {@link OpenedCast.path}: it lets a launch-argument cast be
 * recorded in **Recent** without re-opening it. Resolves to `null` (rather than
 * throwing) in the browser, where there is no such thing as a local path.
 */
export async function currentCastPath(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<string | null>("cast_path");
  } catch {
    // A path is a convenience for the Recent list, never load-bearing.
    return null;
  }
}

/**
 * Tauri-only `quit_now` command — exit the process immediately. Used for a
 * clean File → Quit or confirm-quit path that has no pending guard.
 */
export async function quitNow(): Promise<void> {
  if (!isTauri()) return;
  await invoke<void>("quit_now");
}

/**
 * Tauri-only `request_quit` command — start the dirty-aware quit flow. If the
 * model is dirty the shell emits a `confirm-quit` event; otherwise it exits.
 */
export async function requestQuit(): Promise<void> {
  if (!isTauri()) return;
  await invoke<void>("request_quit");
}

/**
 * Tauri-only `set_dirty` command — report whether the model has unsaved edits.
 */
export function setDirty(dirty: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("set_dirty", { dirty });
}

/**
 * Tauri-only `cancel_quit` command — clear a pending quit/close guard when
 * the user cancels the save-on-quit prompt or a save fails.
 */
export function cancelQuit(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("cancel_quit");
}

/**
 * Tauri-only `confirm_quit` command — complete a close/exit that was
 * previously blocked by the dirty guard.
 */
export function confirmQuit(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("confirm_quit");
}

/**
 * Tauri-only sidecar version probe — runs `agg --version` through the bundled
 * sidecar and returns the first line of stdout. Proves the sidecar invoke path
 * (CASTCU-S2-T04).
 */
export async function aggVersion(): Promise<string> {
  if (!isTauri()) {
    throw new Error("aggVersion is only available in the Tauri desktop shell");
  }
  return invoke<string>("agg_version");
}

/**
 * Tauri-only sidecar version probe — runs `ffmpeg -version` through the bundled
 * sidecar and returns the first line of stdout (CASTCU-S2-T04).
 */
export async function ffmpegVersion(): Promise<string> {
  if (!isTauri()) {
    throw new Error("ffmpegVersion is only available in the Tauri desktop shell");
  }
  return invoke<string>("ffmpeg_version");
}

/**
 * `GET /api/project` (fetch) / `project_meta` command (Tauri) — the launch
 * cast's authoritative `source` filename plus any persisted `.asciicut.json`
 * (for load-on-open, AC#1). The `project` is `null` when nothing has been
 * saved yet.
 */
export function fetchProject(): Promise<ProjectMeta> {
  if (isTauri()) {
    return invoke<ProjectMeta>("project_meta");
  }
  return getJson<ProjectMeta>("/api/project");
}

/**
 * `POST /api/export` — validate + persist the `.asciicut.json` project AND
 * compose + write the composed `.cast`, both to server-derived paths. Returns
 * the two paths written and the composed byte count. The client never sends a
 * path (path-traversal surface lives server-side).
 *
 * **Deliberately `fetch`-only, no Tauri branch.** `/api/export` is out of the
 * `asciicut-bridge` command bridge's scope (CASTCU-S2-T02 plan) — the desktop
 * export story is a different `agg`/`ffmpeg`-backed pipeline landing in a
 * later task, not a bridge of this JSON-project-writer. Calling this inside
 * the desktop shell today would fail (no `/api/*` HTTP surface to `fetch`
 * against, since core editing there runs with no bound TCP port).
 */
export async function exportProject(
  projectJson: string,
): Promise<ExportResponse> {
  return postProjectForJson<ExportResponse>("/api/export", projectJson);
}

/**
 * Tauri-only `export_video` command (CASTCU-S2-T05) — show a native save
 * dialog scoped to `format`, then write the round-trip triple and render the
 * chosen video through the bundled `agg`/`ffmpeg` sidecars, off the UI
 * thread. Streams `export-progress` events throughout (subscribe via
 * {@link onExportProgress} in `lib/desktop.ts`); this promise resolves with
 * the same terminal result the `done` event carries, or rejects.
 *
 * Cancellation — either the save dialog dismissed, or a later
 * {@link cancelExportVideo} call — rejects with an `Error` whose `message`
 * is exactly `"cancelled"`, matching `saveProjectAs`/`openCast`'s convention;
 * callers that want the "quietly did nothing" treatment should catch that
 * specific message rather than getting a `null`, since a promise here also
 * carries the terminal export result.
 *
 * `cutDurationSecs` is the SPA's own already-computed cut duration (the same
 * value `cutTimeLabel()` derives) — used only to scale `ffmpeg`'s
 * fine-grained progress percentage, never trusted for anything else.
 *
 * **Deliberately no browser/fetch fallback.** Like `exportProject` is
 * deliberately fetch-only, this is deliberately Tauri-only: the desktop
 * `agg`/`ffmpeg` sidecar pipeline has no web-demo equivalent (no bundled
 * sidecars to invoke from a browser tab). Callers must branch on `isTauri()`
 * themselves, same as every other desktop-only wrapper in this file.
 */
export async function exportVideo(
  projectJson: string,
  format: ExportVideoFormat,
  cutDurationSecs: number,
): Promise<ExportVideoResult> {
  return invoke<ExportVideoResult>("export_video", {
    project: projectJson,
    format,
    cutDurationSecs,
  });
}

/**
 * Tauri-only `cancel_export` command — kill the currently tracked `agg`/
 * `ffmpeg` sidecar child (if any) so the in-flight {@link exportVideo} call
 * settles with the `"cancelled"` rejection instead of running to completion.
 * A silent no-op if no export is currently running.
 */
export async function cancelExportVideo(): Promise<void> {
  if (!isTauri()) return;
  await invoke<void>("cancel_export");
}
