// Pure segment-model core for the editor's segment track (T11).
//
// This module is DELIBERATELY dependency-free and side-effect-free: no DOM, no
// canvas, no Solid signals. It is the typecheck-covered, inspectable SOURCE OF
// TRUTH for the segment track (AC#3): the immutable `EditModel`, the reducer
// functions that produce new models under clamping invariants, the pure
// hit-testing used by the canvas gesture machine, and the `toProject()`
// projection onto the `.asciicut.json` wire shape. The canvas (`SegmentTrack`)
// and the inputs (`SegmentControls`) are PURE PROJECTIONS: they render `model`
// and mutate only by handing a reducer result back to the Editor.
//
// ─── PROJECT PARITY CONTRACT ─────────────────────────────────────────────────
// `toProject()` MANUALLY MIRRORS the Rust `Project`/`Segment` camelCase JSON
// contract in `crates/asciicut-core/src/compose.rs` (same manual-parity discipline
// `lib/api.ts` documents against `dto.rs`). Rules reproduced from the Rust side:
//   • `#[serde(rename_all = "camelCase")]` → keys are `srcStart`, `srcEnd`,
//     `speed`, `holdEnd`, `idleCap`.
//   • `segments` is an ORDERED playback list — compose iterates it in array order
//     and charges the fixed inter-segment BEAT by index. So array order IS the
//     compose order; `reorder()` (an index move) is semantically distinct from
//     `moveWindow()` (a source-time drag).
//   • `idleCap` defaults to `0.4`; `speed` absent/`0.0` is coerced to `1.0`.
//   • `Project.source` is REQUIRED but the server exposes no endpoint for the
//     launch cast's filename yet — `toProject()` takes it as a parameter and the
//     real filename wiring is the T13 save/load handoff. A placeholder is
//     acceptable for compose, which ignores `source` and uses server state.
// A future OpenAPI/codegen step is the durable fix.
// ─────────────────────────────────────────────────────────────────────────────

/**
 * One keep-segment in the edit model: an inclusive source-time window plus its
 * per-segment playback controls. `id` is a client-only stable handle for
 * selection and reorder — it is NOT projected into the wire shape.
 */
export interface EditSegment {
  /** Stable client id (selection / reorder handle; not projected). */
  readonly id: string;
  /**
   * The segment's stable DISPLAY number — the `1` in `S1` (conflict §4.3).
   *
   * Playback order is editable, so a display id derived from array position
   * would renumber every segment on a reorder and make the inspector's title
   * jump under the user mid-edit. This number is assigned once at creation and
   * never moves: after a re-sequence you legitimately read "S3 plays second",
   * and the contact sheet's own order badge is what states the position.
   *
   * Like {@link id} it is a CLIENT-ONLY handle and is NOT projected onto the
   * wire — `toProject` omits it and `fromProject` re-assigns `1..n` in array
   * order, so the numbering is stable within a session but is not something a
   * hand-written `.asciicut.json` has to carry. Deleting S2 of three leaves
   * `S1, S3`, which is what stable identity means.
   */
  readonly num: number;
  /** Window start (inclusive) in absolute source seconds. */
  readonly srcStart: number;
  /** Window end (inclusive) in absolute source seconds. */
  readonly srcEnd: number;
  /** Playback speed multiplier (`> 0`; inter-event gaps divided by this). */
  readonly speed: number;
  /** End-hold: freeze the last frame this many seconds after the window. */
  readonly holdEnd: number;
  /**
   * Optional human label for the segment. Projected into the wire shape (unlike
   * `id`) — the Rust `Segment` has `label: Option<String>` (`#[serde(default)]`,
   * camelCase), so this survives `export()` (SPEC §8.4/§8.5). Absent when unset.
   */
  readonly label?: string;
}

/**
 * The immutable edit model — the single source of truth for the segment track.
 * `segments` is an ordered playback list (array order = compose order).
 */
export interface EditModel {
  /** The ordered keep-segments (array order is the compose/playback order). */
  readonly segments: readonly EditSegment[];
  /** Global idle cap in seconds (`>= 0`). */
  readonly idleCap: number;
}

/** A hit-test region on a segment (used by the canvas gesture machine). */
export type HitRegion = "left" | "right" | "body";

/** A resolved hit: which segment and where on it the pointer landed. */
export interface Hit {
  /** The stable id of the segment under the pointer. */
  readonly id: string;
  /** Which part of the segment: an edge handle (`left`/`right`) or the body. */
  readonly region: HitRegion;
}

/** The minimum segment window width (seconds) — a window never collapses to 0. */
export const MIN_SEGMENT_SECS = 0.1;

/**
 * The minimum playback speed. The Rust engine coerces `0.0` → `1.0` and rejects
 * negatives, and dividing inter-event gaps by a near-zero speed explodes the
 * timeline, so the UI clamps to a concrete positive floor (review advisory).
 */
export const MIN_SPEED = 0.1;

/** The default per-segment playback speed (matches the Rust `default_speed`). */
export const DEFAULT_SPEED = 1.0;

/** The default global idle cap (matches the Rust `DEFAULT_IDLE_CAP`). */
export const DEFAULT_IDLE_CAP = 0.4;

/** Clamp `v` into `[lo, hi]` (assumes `lo <= hi`). */
function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

/** A monotonic client-id source — deterministic within a session, DOM-free. */
let idCounter = 0;
function nextId(): string {
  idCounter += 1;
  return `seg-${idCounter}`;
}

/**
 * The next free display number for `model` — one past the highest in use, so a
 * number is never reused while an older segment still shows it. Starts at 1 for
 * an empty model, which is why `num` is derived from the model rather than from
 * a module counter like {@link nextId}: a freshly opened project always reads
 * `S1, S2, …` regardless of how much editing happened earlier in the session.
 */
function nextNum(model: EditModel): number {
  return model.segments.reduce((max, seg) => Math.max(max, seg.num), 0) + 1;
}

/** The segment's display tag (`S3`) — the one place that spelling is defined. */
export function segmentTag(seg: EditSegment): string {
  return `S${seg.num}`;
}

/** An empty model with the given (or default) idle cap. */
export function emptyModel(idleCap: number = DEFAULT_IDLE_CAP): EditModel {
  return { segments: [], idleCap: Math.max(0, idleCap) };
}

/** Replace the segment with matching `id` via `fn`, returning a new model. */
function mapSegment(
  model: EditModel,
  id: string,
  fn: (seg: EditSegment) => EditSegment,
): EditModel {
  let changed = false;
  const segments = model.segments.map((seg) => {
    if (seg.id !== id) return seg;
    changed = true;
    return fn(seg);
  });
  return changed ? { ...model, segments } : model;
}

/**
 * Create (draw) a new segment from a raw source-time span. The span is
 * normalized (drawing right-to-left is fine), clamped into `[0, duration]`, and
 * widened to at least {@link MIN_SEGMENT_SECS}. The new segment is APPENDED
 * (last in playback order) with default speed / hold. Returns the new model and
 * the new segment's id so the caller can select it.
 */
export function createSegment(
  model: EditModel,
  rawA: number,
  rawB: number,
  duration: number,
  label?: string,
): { model: EditModel; id: string } {
  const max = Math.max(duration, MIN_SEGMENT_SECS);
  let start = clamp(Math.min(rawA, rawB), 0, max);
  let end = clamp(Math.max(rawA, rawB), 0, max);
  if (end - start < MIN_SEGMENT_SECS) {
    // Grow to the min width without escaping [0, max].
    end = Math.min(max, start + MIN_SEGMENT_SECS);
    start = Math.max(0, end - MIN_SEGMENT_SECS);
  }
  const seg: EditSegment = {
    id: nextId(),
    num: nextNum(model),
    srcStart: start,
    srcEnd: end,
    speed: DEFAULT_SPEED,
    holdEnd: 0,
    // Omit the key entirely when unlabelled so it never lands as `undefined`.
    ...(label !== undefined ? { label } : {}),
  };
  return { model: { ...model, segments: [...model.segments, seg] }, id: seg.id };
}

/**
 * Drag a segment's WINDOW along source time by `deltaSecs`, preserving its
 * width and clamping the whole window inside `[0, duration]`. This moves the
 * source window — it does NOT change compose order (that is {@link reorder}).
 */
export function moveWindow(
  model: EditModel,
  id: string,
  deltaSecs: number,
  duration: number,
): EditModel {
  const max = Math.max(duration, MIN_SEGMENT_SECS);
  return mapSegment(model, id, (seg) => {
    const width = seg.srcEnd - seg.srcStart;
    const start = clamp(seg.srcStart + deltaSecs, 0, Math.max(0, max - width));
    return { ...seg, srcStart: start, srcEnd: start + width };
  });
}

/**
 * Resize a segment by dragging its `left` or `right` edge handle to a source
 * time, keeping `srcStart + MIN_SEGMENT_SECS <= srcEnd` and staying inside
 * `[0, duration]`.
 */
export function resizeEdge(
  model: EditModel,
  id: string,
  edge: "left" | "right",
  timeSecs: number,
  duration: number,
): EditModel {
  const max = Math.max(duration, MIN_SEGMENT_SECS);
  return mapSegment(model, id, (seg) => {
    if (edge === "left") {
      const start = clamp(timeSecs, 0, seg.srcEnd - MIN_SEGMENT_SECS);
      return { ...seg, srcStart: start };
    }
    const end = clamp(timeSecs, seg.srcStart + MIN_SEGMENT_SECS, max);
    return { ...seg, srcEnd: end };
  });
}

/**
 * Move a segment from `fromIndex` to `toIndex` in the ordered playback list.
 * Out-of-range indices are clamped; a no-op move returns the same model.
 */
export function reorder(
  model: EditModel,
  fromIndex: number,
  toIndex: number,
): EditModel {
  const n = model.segments.length;
  const from = clamp(Math.trunc(fromIndex), 0, n - 1);
  const to = clamp(Math.trunc(toIndex), 0, n - 1);
  if (n === 0 || from === to) return model;
  const segments = model.segments.slice();
  const [moved] = segments.splice(from, 1);
  segments.splice(to, 0, moved);
  return { ...model, segments };
}

/** Convenience: shift the segment with `id` one step earlier/later in order. */
export function moveByStep(
  model: EditModel,
  id: string,
  step: -1 | 1,
): EditModel {
  const idx = model.segments.findIndex((s) => s.id === id);
  if (idx < 0) return model;
  return reorder(model, idx, idx + step);
}

/** Remove the segment with `id` (a no-op if it is not present). */
export function remove(model: EditModel, id: string): EditModel {
  const segments = model.segments.filter((s) => s.id !== id);
  return segments.length === model.segments.length
    ? model
    : { ...model, segments };
}

/** Set a segment's playback speed, clamped to a positive floor. */
export function setSpeed(model: EditModel, id: string, speed: number): EditModel {
  const s = Number.isFinite(speed) ? Math.max(MIN_SPEED, speed) : DEFAULT_SPEED;
  return mapSegment(model, id, (seg) => ({ ...seg, speed: s }));
}

/** Set a segment's end-hold (seconds), clamped to `>= 0`. */
export function setHoldEnd(
  model: EditModel,
  id: string,
  holdEnd: number,
): EditModel {
  const h = Number.isFinite(holdEnd) ? Math.max(0, holdEnd) : 0;
  return mapSegment(model, id, (seg) => ({ ...seg, holdEnd: h }));
}

/** Set the global idle cap (seconds), clamped to `>= 0`. */
export function setIdleCap(model: EditModel, idleCap: number): EditModel {
  const c = Number.isFinite(idleCap) ? Math.max(0, idleCap) : DEFAULT_IDLE_CAP;
  return { ...model, idleCap: c };
}

/**
 * Pure hit-test: which segment (and which part) sits under a source `time`.
 *
 * `edgeTolSecs` is the edge-handle grab tolerance in seconds (the caller
 * converts an edge pixel width through the current view). Edge handles win over
 * the body. Segments are tested in REVERSE array order so the last-drawn (top-
 * most) segment resolves an overlap deterministically. Returns `null` on empty
 * space.
 */
export function hitTest(
  model: EditModel,
  time: number,
  edgeTolSecs: number,
): Hit | null {
  for (let i = model.segments.length - 1; i >= 0; i--) {
    const seg = model.segments[i];
    if (time < seg.srcStart - edgeTolSecs || time > seg.srcEnd + edgeTolSecs) {
      continue;
    }
    if (Math.abs(time - seg.srcStart) <= edgeTolSecs) {
      return { id: seg.id, region: "left" };
    }
    if (Math.abs(time - seg.srcEnd) <= edgeTolSecs) {
      return { id: seg.id, region: "right" };
    }
    if (time >= seg.srcStart && time <= seg.srcEnd) {
      return { id: seg.id, region: "body" };
    }
  }
  return null;
}

/** A projected segment on the wire — the camelCase `Segment` fields only. */
export interface ProjectSegmentDto {
  srcStart: number;
  srcEnd: number;
  speed: number;
  holdEnd: number;
  /** Optional human label — mirrors Rust `Segment.label`; omitted when unset. */
  label?: string;
}

/**
 * The `output` block on the wire — mirrors the Rust `Output` camelCase shape.
 * Carried as OPAQUE pass-through project metadata (T13): the editor never edits
 * these fields, but `compose()` folds `width`/`height` into the composed header,
 * so they MUST survive load→edit→save (the AC#1 exact-edit-state invariant).
 * `theme` is an arbitrary JSON value on the Rust side; kept `unknown` here.
 */
export interface OutputDto {
  width?: number;
  height?: number;
  theme?: unknown;
  fontSize?: number;
}

/**
 * A marker on the wire — mirrors the Rust `Marker` shape (NOT camelCase-renamed
 * on the Rust side: plain `t` + optional `text`). Carried as opaque pass-through
 * metadata so markers round-trip through load→save (AC#1) even though `compose()`
 * does not yet project them into output.
 */
export interface MarkerDto {
  t: number;
  text?: string;
}

/** The projected project on the wire — mirrors the Rust `Project` camelCase shape. */
export interface ProjectDto {
  source: string;
  /** Opaque pass-through output block; omitted when absent (DTO parity). */
  output?: OutputDto;
  idleCap: number;
  segments: ProjectSegmentDto[];
  /** Opaque pass-through marker list; omitted when absent (DTO parity). */
  markers?: MarkerDto[];
}

/** The opaque non-segment project metadata carried alongside the `EditModel`. */
export interface ProjectExtras {
  /** Opaque pass-through output geometry (folded into the composed header). */
  readonly output?: OutputDto;
  /** Opaque pass-through marker list (round-trips through load→save). */
  readonly markers?: readonly MarkerDto[];
}

/**
 * Project the edit model onto the `.asciicut.json` wire shape (see the PROJECT
 * PARITY CONTRACT above). Array order is preserved (it is the compose order).
 * `source` is supplied by the caller — the T13 handoff — and MUST be a real
 * filename before persistence; a placeholder is acceptable only for compose.
 * `extras` carries the opaque `output`/`markers` metadata straight through
 * (omit-when-absent parity) so the projection is a faithful inverse of
 * {@link fromProject}.
 */
export function toProject(
  model: EditModel,
  source: string,
  extras?: ProjectExtras,
): ProjectDto {
  const dto: ProjectDto = {
    source,
    idleCap: model.idleCap,
    segments: model.segments.map((seg) => {
      const segDto: ProjectSegmentDto = {
        srcStart: seg.srcStart,
        srcEnd: seg.srcEnd,
        speed: seg.speed,
        holdEnd: seg.holdEnd,
      };
      // Project the label only when present (omit-when-absent DTO parity).
      if (seg.label !== undefined) segDto.label = seg.label;
      return segDto;
    }),
  };
  // Splice the opaque metadata back in only when present; `output` sits before
  // `idleCap`/`segments` on the wire but key order is JSON-irrelevant here — the
  // Rust decoder is order-independent and compose is the byte-identity oracle.
  if (extras?.output !== undefined) dto.output = extras.output;
  if (extras?.markers !== undefined) dto.markers = [...extras.markers];
  return dto;
}

/**
 * The pure inverse of {@link toProject}: hydrate an {@link EditModel} (plus the
 * `source` and opaque `output`/`markers` metadata) from a decoded `ProjectDto`.
 * Each segment is assigned a FRESH client id (ids are client-only handles, never
 * on the wire), so a reload produces stable-but-new selection handles. Missing
 * `speed`/`holdEnd` fall back to the model defaults (mirroring the Rust
 * `#[serde(default)]` coercions) so a hand-written project still hydrates.
 */
export function fromProject(dto: ProjectDto): {
  model: EditModel;
  source: string;
  output?: OutputDto;
  markers?: MarkerDto[];
} {
  const segments: EditSegment[] = (dto.segments ?? []).map((seg, i) => {
    const speed =
      Number.isFinite(seg.speed) && seg.speed > 0 ? seg.speed : DEFAULT_SPEED;
    const holdEnd = Number.isFinite(seg.holdEnd) ? Math.max(0, seg.holdEnd) : 0;
    const out: EditSegment = {
      id: nextId(),
      // Display numbers are not on the wire; a loaded project reads `S1..Sn`
      // in array order and is stable from there (see `EditSegment.num`).
      num: i + 1,
      srcStart: seg.srcStart,
      srcEnd: seg.srcEnd,
      speed,
      holdEnd,
      ...(seg.label !== undefined ? { label: seg.label } : {}),
    };
    return out;
  });
  const idleCap = Number.isFinite(dto.idleCap)
    ? Math.max(0, dto.idleCap)
    : DEFAULT_IDLE_CAP;
  const result: {
    model: EditModel;
    source: string;
    output?: OutputDto;
    markers?: MarkerDto[];
  } = {
    model: { segments, idleCap },
    source: dto.source,
  };
  if (dto.output !== undefined) result.output = dto.output;
  if (dto.markers !== undefined) result.markers = dto.markers;
  return result;
}
