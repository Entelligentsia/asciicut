// Automation seam groundwork (SPEC §8.4).
//
// E2E/automation tasks drive the SPA through a stable `window.asciicut.*` API and
// `data-testid` hooks rather than brittle DOM selectors. This module owns the
// global surface: the read-only snapshots (`timeline`/`player`/`segments`) that
// mirror opaque canvas/player state for assertions, AND (T12) the imperative
// SPEC §8.4 command API (`open`/`addSegment`/`select`/`setSpeed`/`setHold`/
// `setIdleCap`/`getProject`/`export`) wired to the shared `editorStore` and the
// `lib/api` client. Both live on the one `window.asciicut` object.

import { exportProject, type ExportResponse } from "./api";
import { editorStore, type AddSegmentInput } from "./editorStore";
import type { ProjectDto } from "./segments";

/**
 * Read-only snapshot of the activity-timeline state (T09). A `<canvas>` is
 * opaque to the a11y snapshot, so this is the honest verification hook (AC#3):
 * E2E can assert the playhead, view range, and loaded bucket count without
 * pixel-reading the canvas. `null` until the editor view has loaded the signal.
 */
export interface TimelineState {
  /** The playhead time in seconds. */
  readonly playhead: number;
  /** The visible window `[start, end]` in seconds. */
  readonly view: readonly [number, number];
  /** The number of waveform buckets loaded. */
  readonly bucketCount: number;
}

/**
 * Read-only snapshot of the in-memory player preview (T10). The asciinema
 * player renders into an opaque `<div>` that the a11y snapshot cannot inspect,
 * so — exactly like the T09 canvas — this is the honest verification hook
 * (AC#1): E2E can assert the player mounted from the dropped cast and that a
 * `seek()` was applied, without pixel-reading the terminal. `null` whenever no
 * recording is mounted (empty/error state).
 */
export interface PlayerState {
  /** True while a player instance is mounted from an in-memory cast. */
  readonly mounted: boolean;
  /** Byte length of the in-memory cast text feeding the player. */
  readonly bytes: number;
  /** The last seek target in seconds applied via the adapter, or `null`. */
  readonly lastSeek: number | null;
}

/**
 * Read-only snapshot of the editor's segment track (T11). The segment track is
 * a `<canvas>` — opaque to the a11y snapshot — so, exactly like the T09 timeline
 * and T10 player seams, this is the honest verification hook (AC#3): E2E can
 * assert the segment count, the selection, the global idle cap, and each
 * segment's window/controls without pixel-reading the canvas. Mirrors the
 * immutable `EditModel` source of truth. `null` until the editor has loaded.
 */
export interface SegmentsState {
  /** The number of segments in the model. */
  readonly count: number;
  /** The stable id of the selected segment, or `null`. */
  readonly selectedId: string | null;
  /** The global idle cap in seconds. */
  readonly idleCap: number;
  /** The ordered segments (array order is the compose order). */
  readonly segments: readonly {
    readonly id: string;
    readonly srcStart: number;
    readonly srcEnd: number;
    readonly speed: number;
    readonly holdEnd: number;
  }[];
}

export interface AsciicutAutomation {
  /** Semver of the SPA build, for E2E assertions and diagnostics. */
  readonly version: string;
  /** True once the app has mounted — E2E can poll this for readiness. */
  ready: boolean;
  /** Read-only timeline state; `null` until the editor loads (T09). */
  timeline: TimelineState | null;
  /** Read-only player-preview state; `null` until a cast is mounted (T10). */
  player: PlayerState | null;
  /** Read-only segment-track state; `null` until the editor loads (T11). */
  segments: SegmentsState | null;

  // ─── Imperative command surface (SPEC §8.4, T12) ───────────────────────────
  /**
   * Record the project `source` name (optional) and navigate to the editor
   * route, resolving once the editor has mounted and published its `segments`
   * snapshot. On mount the editor authoritatively loads the launch cast's
   * `source` and any persisted `.asciicut.json` via `GET /api/project` (T13
   * load-on-open), so a passed `source` is only an early hint — the server value
   * wins. The Sprint-1 server still has no arbitrary-cast open endpoint: compose
   * always runs against the launch cast.
   */
  open(source?: string): Promise<void>;
  /** Append a segment (optional speed/hold/label); returns the new segment id. */
  addSegment(input: AddSegmentInput): string;
  /** Select a segment by id (`null` clears the selection). */
  select(id: string | null): void;
  /** Set the selected segment's speed; throws if nothing is selected. */
  setSpeed(v: number): void;
  /** Set the selected segment's end-hold; throws if nothing is selected. */
  setHold(v: number): void;
  /** Set the global idle cap (seconds). */
  setIdleCap(v: number): void;
  /** Return the in-memory `.asciicut.json` project (SPEC §8.4). */
  getProject(): ProjectDto;
  /**
   * Export the project via `POST /api/export`, resolving with the
   * {@link ExportResponse}: the server writes BOTH the `.asciicut.json` project
   * AND the composed `.cast` to server-derived paths, and returns both paths
   * plus the composed byte count (AC#2). Video/`{format:'mp4'}` rendering has NO
   * server endpoint in Sprint 1 — the `mp4` hint is accepted but the render step
   * is a documented Sprint-2 deferral (SPEC §9); `format:'cast'` (the default) is
   * emitted for real. The projected `label`/`output`/`markers` round-trip
   * verbatim into the persisted `.asciicut.json`.
   */
  export(opts?: ExportOptions): Promise<ExportResponse>;
}

/** Options for {@link AsciicutAutomation.export}. `mp4` is a Sprint-1 deferral. */
export interface ExportOptions {
  /** Output format hint; `mp4` is accepted but not rendered in Sprint 1. */
  readonly format?: "cast" | "mp4";
}

declare global {
  interface Window {
    asciicut?: AsciicutAutomation;
  }
}

/**
 * Poll `pred` until it is true (or `timeoutMs` elapses). Used by `open()` to
 * resolve on editor readiness. Advisory #2/#3: `open()` must resolve on the
 * `segments` snapshot going live (editor mounted + signal loaded), NOT on
 * `ready`, which flips at app mount before the editor route is active.
 */
function waitFor(
  pred: () => boolean,
  timeoutMs = 5000,
  intervalMs = 25,
): Promise<void> {
  return new Promise((resolve, reject) => {
    if (pred()) {
      resolve();
      return;
    }
    const start = Date.now();
    const timer = setInterval(() => {
      if (pred()) {
        clearInterval(timer);
        resolve();
      } else if (Date.now() - start > timeoutMs) {
        clearInterval(timer);
        reject(
          new Error("asciicut: open() timed out waiting for the editor to load"),
        );
      }
    }, intervalMs);
  });
}

/** Install the `window.asciicut` automation surface. Idempotent. */
export function installAutomation(version: string): AsciicutAutomation {
  const existing = window.asciicut;
  if (existing) {
    return existing;
  }
  const api: AsciicutAutomation = {
    version,
    ready: false,
    timeline: null,
    player: null,
    segments: null,
    async open(source?: string): Promise<void> {
      if (source !== undefined) editorStore.setSource(source);
      if (window.location.hash !== "#/" && window.location.hash !== "") {
        window.location.hash = "#/";
      }
      await waitFor(() => window.asciicut?.segments != null);
    },
    addSegment(input: AddSegmentInput): string {
      return editorStore.addSegment(input);
    },
    select(id: string | null): void {
      editorStore.select(id);
    },
    setSpeed(v: number): void {
      editorStore.setSelectedSpeed(v);
    },
    setHold(v: number): void {
      editorStore.setSelectedHold(v);
    },
    setIdleCap(v: number): void {
      editorStore.setIdleCap(v);
    },
    getProject(): ProjectDto {
      return editorStore.getProject();
    },
    async export(opts?: ExportOptions): Promise<ExportResponse> {
      // `format` is accepted for forward-compat but mp4 rendering has no Sprint-1
      // server endpoint; export writes the `.asciicut.json` + composed `.cast`
      // (SPEC §9 deferral covers video only).
      void opts;
      return exportProject(JSON.stringify(editorStore.getProject()));
    },
  };
  window.asciicut = api;
  return api;
}

/**
 * Publish (or clear) the read-only timeline state on `window.asciicut`. A no-op
 * if the automation stub is not installed. Called by the editor view whenever
 * the playhead / viewport / signal changes.
 */
export function setTimelineState(state: TimelineState | null): void {
  if (window.asciicut) window.asciicut.timeline = state;
}

/**
 * Publish (or clear) the read-only player-preview state on `window.asciicut`. A
 * no-op if the automation stub is not installed. Called by `<PlayerPreview>`
 * whenever a recording mounts, a seek applies, or the player tears down.
 */
export function setPlayerState(state: PlayerState | null): void {
  if (window.asciicut) window.asciicut.player = state;
}

/**
 * Publish (or clear) the read-only segment-track state on `window.asciicut`. A
 * no-op if the automation stub is not installed. Called by the editor view
 * whenever the segment model or the selection changes (T11).
 */
export function setSegmentsState(state: SegmentsState | null): void {
  if (window.asciicut) window.asciicut.segments = state;
}
