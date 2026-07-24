// Pure segment-schedule helpers for the editor transport + playhead (prototype
// `segStartInCut` / `segOut` / `cutDuration` port).
//
// The compose engine lives in Rust (`asciicut-core::compose`) and runs
// server-side; the SPA has no event stream to compute the exact per-segment
// "active length" (seconds of real change that survive the idle cap). For the
// transport clock + playhead mapping we only need an APPROXIMATE cut timeline:
// each segment's on-screen duration is approximated as `(window / speed) +
// holdEnd`, with the fixed inter-segment `BEAT` (0.5s) charged before every
// non-first segment — mirroring `compose.rs`'s index-based beat. The real
// composed `.cast` the asciinema-player plays is the byte-identity oracle
// (T05/T13); this module only drives the visual playhead + header cut-time +
// "now playing" segment, so an approximate schedule is honest and sufficient.
//
// Pure + side-effect-free (no DOM, no Solid): typecheck-covered and inspectable.

import { BEAT } from "./beat";
import type { EditModel, EditSegment } from "./segments";

/** One segment's place on the approximate cut timeline. */
export interface ScheduledSeg {
  /** The segment's stable client id. */
  readonly id: string;
  /** The segment's stable display number (the `1` in `S1`). */
  readonly num: number;
  /** Source window start (inclusive, seconds). */
  readonly srcStart: number;
  /** Source window end (inclusive, seconds). */
  readonly srcEnd: number;
  /** Speed multiplier. */
  readonly speed: number;
  /** End-hold seconds. */
  readonly holdEnd: number;
  /** Optional human label. */
  readonly label?: string;
  /** Cut-timeline start (seconds). */
  readonly cutStart: number;
  /** On-screen duration on the cut timeline (seconds, approximate). */
  readonly cutDur: number;
}

/** The approximate cut-timeline schedule for a model. */
export interface Schedule {
  /** One entry per segment, in compose (array) order. */
  readonly segments: readonly ScheduledSeg[];
  /** Total approximate cut duration (seconds). */
  readonly total: number;
}

/**
 * A positive-speed floor. A segment's speed divides source time, so a zero or
 * negative value would send the schedule to infinity; the model clamps speed on
 * entry (`segments.MIN_SPEED`) and this is the belt-and-braces guard for a
 * hand-written project that slipped through.
 */
const MIN_EFFECTIVE_SPEED = 1e-6;

/**
 * The part of a scheduled segment's cut duration during which source time
 * actually advances — its `cutDur` minus the frozen `holdEnd` tail. Every
 * cut↔source mapping below pivots on this, so it lives in one place.
 */
function motionDurOf(seg: ScheduledSeg): number {
  return Math.max(0, seg.cutDur - Math.max(0, seg.holdEnd));
}

/**
 * Build the approximate cut schedule for an edit model. Mirrors the prototype's
 * `segOut`/`cutDuration` math with `activeLen ≈ window length` and the fixed
 * `BEAT` before every non-first segment. Empty windows contribute nothing
 * (matching `compose.rs`'s `if window.is_empty() { continue }` — though here a
 * zero-width window is the only "empty" case, since we have no event stream).
 */
export function scheduleOf(model: EditModel): Schedule {
  const segs: ScheduledSeg[] = [];
  let clock = 0;
  model.segments.forEach((seg, i) => {
    if (i > 0) clock += BEAT;
    const window = Math.max(0, seg.srcEnd - seg.srcStart);
    const cutDur =
      window / Math.max(seg.speed, MIN_EFFECTIVE_SPEED) + Math.max(0, seg.holdEnd);
    segs.push({
      id: seg.id,
      num: seg.num,
      srcStart: seg.srcStart,
      srcEnd: seg.srcEnd,
      speed: seg.speed,
      holdEnd: seg.holdEnd,
      ...(seg.label !== undefined ? { label: seg.label } : {}),
      cutStart: clock,
      cutDur,
    });
    clock += cutDur;
  });
  return { segments: segs, total: clock };
}

/**
 * Map a cut-timeline time to the source segment playing at that moment, plus the
 * source-time position inside it. Returns `null` when there are no segments or
 * the cut-time is before the first segment. Clamps to the last segment at the
 * tail (so the playhead parks on the final frame once the cut finishes).
 */
export function cutToSource(
  sched: Schedule,
  cutTime: number,
): { seg: ScheduledSeg; srcTime: number } | null {
  const segs = sched.segments;
  if (segs.length === 0) return null;
  if (cutTime <= 0) {
    const s = segs[0];
    return { seg: s, srcTime: s.srcStart };
  }
  for (const s of segs) {
    if (cutTime >= s.cutStart && cutTime < s.cutStart + s.cutDur) {
      // HoldEnd occupies the tail of cutDur as a frozen last frame, so only the
      // pre-hold portion advances source time.
      const motionDur = motionDurOf(s);
      const motionFrac = motionDur > 0 ? Math.min(1, (cutTime - s.cutStart) / motionDur) : 1;
      const srcTime = s.srcStart + motionFrac * (s.srcEnd - s.srcStart);
      return { seg: s, srcTime };
    }
  }
  // Past the end: park on the last segment's final frame.
  const last = segs[segs.length - 1];
  return { seg: last, srcTime: last.srcEnd };
}

/** Find a segment's index by id (or -1). */
export function indexOfId(sched: Schedule, id: string | null): number {
  if (id === null) return -1;
  return sched.segments.findIndex((s) => s.id === id);
}

/**
 * Map a source time to the approximate cut-timeline time (the inverse of
 * {@link cutToSource}). Used so a source-timeline scrub/seek can drive the
 * composed-cut player: inside a segment, `cut = cutStart + (src - srcStart) *
 * speed`, capped at the motion duration (the hold tail maps to the segment's
 * final cut time). Outside every segment, snaps to the nearest segment edge so
 * seeking dead air lands on an adjacent cut boundary rather than mid-gap.
 */
export function sourceToCut(sched: Schedule, srcTime: number): number {
  const segs = sched.segments;
  if (segs.length === 0) return 0;
  for (const s of segs) {
    if (srcTime >= s.srcStart && srcTime <= s.srcEnd) {
      const motionDur = motionDurOf(s);
      const advanced =
        (srcTime - s.srcStart) * Math.max(s.speed, MIN_EFFECTIVE_SPEED);
      return s.cutStart + Math.min(motionDur, advanced);
    }
  }
  // Outside all windows: snap to the nearest segment edge in cut time.
  let best = 0;
  let bestDist = Infinity;
  for (const s of segs) {
    for (const edge of [s.srcStart, s.srcEnd]) {
      const d = Math.abs(srcTime - edge);
      if (d < bestDist) {
        bestDist = d;
        best = edge === s.srcStart ? s.cutStart : s.cutStart + motionDurOf(s);
      }
    }
  }
  return best;
}

/** Select-adjacent navigation helper: the id after the current, or null. */
export function nextId(sched: Schedule, id: string | null): string | null {
  const i = indexOfId(sched, id);
  if (i < 0) return sched.segments[0]?.id ?? null;
  return sched.segments[Math.min(sched.segments.length - 1, i + 1)]?.id ?? null;
}

/** Select-adjacent navigation helper: the id before the current, or null. */
export function prevId(sched: Schedule, id: string | null): string | null {
  const i = indexOfId(sched, id);
  if (i <= 0) return sched.segments[0]?.id ?? null;
  return sched.segments[Math.max(0, i - 1)]?.id ?? null;
}

/** The source segment whose window contains `srcTime`, or null (for filmstrip). */
export function segmentAtSource(
  model: EditModel,
  srcTime: number,
): EditSegment | null {
  for (let i = model.segments.length - 1; i >= 0; i--) {
    const s = model.segments[i];
    if (srcTime >= s.srcStart && srcTime <= s.srcEnd) return s;
  }
  return null;
}