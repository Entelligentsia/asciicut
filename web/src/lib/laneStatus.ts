// What is the source showing at time `t`, and is any segment keeping it?
//
// Backs the activity-lane hover guide and the source-inspection badge (brief
// §3H): the histogram becomes a *scanning* tool only if, before clicking, you
// can tell whether a moment is kept, genuinely idle, or real activity that no
// segment covers. Those three cases are exactly what turns "inspect an
// unsegmented stretch" from a curiosity into the action `✂ cut here`.
//
// Pure + side-effect-free (no DOM, no Solid).

import type { ActivitySignalDto } from "./api";

/** The three things a source moment can be, relative to the current cut. */
export type LaneStatusKind = "kept" | "unkept" | "dead";

/** A resolved lane status: its kind plus a short human tag. */
export interface LaneStatus {
  /** kept = inside a segment · unkept = real activity no segment covers · dead = idle. */
  readonly kind: LaneStatusKind;
  /** A display tag: the segment number when kept, else a phrase. */
  readonly tag: string;
}

/**
 * The activity score at source time `t` — the bucket `t` falls in, or 0 when
 * `t` is out of range. The bucket span is `bucket_secs`, so `t / bucket_secs`
 * is the index.
 */
export function activityAt(signal: ActivitySignalDto, t: number): number {
  if (signal.bucket_secs <= 0) return 0;
  const i = Math.floor(t / signal.bucket_secs);
  return i >= 0 && i < signal.buckets.length ? signal.buckets[i] : 0;
}

/**
 * Classify source time `t`.
 *
 * `keptTag(t)` returns the display tag of the segment covering `t` (e.g. `S2`)
 * or `null` if none does — kept beats everything. Otherwise a bucket score at
 * or above `activeThreshold` of the signal's peak is "unkept activity" (a real
 * change no segment keeps — the thing worth cutting to), and anything quieter
 * is "dead air".
 */
export function laneStatusAt(
  signal: ActivitySignalDto,
  t: number,
  keptTag: (t: number) => string | null,
  peak: number,
  activeThreshold = 0.28,
): LaneStatus {
  const kept = keptTag(t);
  if (kept !== null) return { kind: "kept", tag: kept };
  const score = activityAt(signal, t);
  if (peak > 0 && score >= activeThreshold * peak) {
    return { kind: "unkept", tag: "unkept activity" };
  }
  return { kind: "dead", tag: "dead air" };
}
