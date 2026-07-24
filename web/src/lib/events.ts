// Event-time snapping for the IN/OUT nudge (brief §3D).
//
// A segment boundary is only meaningful where the recording actually changed.
// Nudging by a fixed step lands mid-gap, on a frame identical to its neighbour,
// so the user cannot tell the nudge did anything. Snapping to the cast's own
// event timestamps — served by `GET /api/events` / the `event_times` command,
// both over `asciicut_bridge::ops::event_times` — makes the step *the honest
// quantum*: every nudge lands on a frame where something happened.
//
// Pure + side-effect-free (no DOM, no Solid, no fetch): the list is loaded once
// by the editor and passed in.

/**
 * Times closer together than this are the same moment. Guards the `> t` /
 * `< t` searches against floating-point equality: without it, snapping from a
 * boundary that already sits exactly on an event could return that same event
 * and the nudge would be a silent no-op.
 */
const EPSILON = 1e-6;

/**
 * Index of the first time strictly greater than `t`, or `times.length` when
 * `t` is at or past the last event. Binary search — the sample cast has tens of
 * thousands of events and nudging is a per-keystroke operation.
 *
 * `times` MUST be ascending (the bridge op guarantees it).
 */
function upperBound(times: readonly number[], t: number): number {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] > t + EPSILON) {
      hi = mid;
    } else {
      lo = mid + 1;
    }
  }
  return lo;
}

/**
 * The next real event time strictly after `t`. Returns `t` unchanged when there
 * is none (already at the tail, or an empty event list) — so a nudge at the end
 * of the recording is a no-op rather than a jump to nowhere.
 */
export function snapNext(times: readonly number[], t: number): number {
  const i = upperBound(times, t);
  return i < times.length ? times[i] : t;
}

/**
 * The previous real event time strictly before `t`, or `t` unchanged when there
 * is none. The mirror of {@link snapNext}.
 */
export function snapPrev(times: readonly number[], t: number): number {
  // `upperBound` is the first index > t; the first index < t is therefore at
  // most `i - 1`, stepping back over any run that ties with `t` itself.
  let i = upperBound(times, t) - 1;
  while (i >= 0 && times[i] >= t - EPSILON) i--;
  return i >= 0 ? times[i] : t;
}

/**
 * Snap `n` events away from `t` — `n > 0` forward, `n < 0` back. The coarse
 * keyboard step (a modifier on the nudge keys) is "several events at once",
 * not "a bigger number of seconds": it stays on the event grid, so the
 * guarantee that a boundary sits on a real frame holds at every step size.
 */
export function snapBy(times: readonly number[], t: number, n: number): number {
  let out = t;
  const step = n > 0 ? snapNext : snapPrev;
  for (let i = 0; i < Math.abs(n); i++) {
    const next = step(times, out);
    // Ran off the end — stop rather than spinning on an unchanging value.
    if (next === out) break;
    out = next;
  }
  return out;
}

/**
 * The event time nearest `t` (ties resolve forward). Used when an existing
 * boundary — drawn by dragging on the lane, or loaded from a hand-written
 * project — needs to be pulled onto the grid before nudging from it.
 */
export function snapNearest(times: readonly number[], t: number): number {
  if (times.length === 0) return t;
  const i = upperBound(times, t);
  if (i === 0) return times[0];
  if (i >= times.length) return times[times.length - 1];
  const before = times[i - 1];
  const after = times[i];
  return t - before <= after - t ? before : after;
}
