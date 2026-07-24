// Pure viewport / mapping / aggregation helpers for the activity timeline.
//
// This module is DELIBERATELY dependency-free and side-effect-free: no DOM, no
// canvas, no Solid signals. It is the typecheck-covered, inspectable core of the
// timeline feature — the time↔pixel mapping, the zoom-around-cursor and
// clamp-pan math, and the per-pixel-column bucket aggregation the canvas draws.
// Keeping it pure means the interaction math can be reasoned about (and later
// unit-tested) in isolation from the canvas rendering in Timeline.tsx.

import type { ActivitySignalDto, ThumbDto } from "./api";

/** A visible time window, in source seconds. `end > start` for a valid view. */
export interface Viewport {
  /** Left edge of the visible window, in seconds. */
  start: number;
  /** Right edge of the visible window, in seconds. */
  end: number;
}

/**
 * The smallest view span we allow (seconds). Zooming in past this is clamped so
 * the mapping never collapses to a zero-width window (which would divide by 0).
 */
export const MIN_VIEW_SECS = 0.25;

/**
 * The waveform's OWN span: `buckets.length * bucket_secs`. This is ceil-quantized
 * — it can be up to one bucket longer than the last-event time — so it is NOT
 * the canonical duration for filmstrip alignment (see {@link canonicalDuration}).
 */
export function signalDuration(signal: ActivitySignalDto): number {
  return signal.buckets.length * signal.bucket_secs;
}

/**
 * The ONE canonical view duration fed to both the waveform and the filmstrip.
 *
 * Prefers the last thumbnail's source time (the last-event basis `/api/thumbs`
 * uses for its `t` values) so a filmstrip cell never maps beyond the right edge;
 * falls back to the ceil-quantized {@link signalDuration} when there are no
 * thumbs. Deriving a single basis here avoids a systematic sub-bucket filmstrip
 * skew at the right edge (AC#2). Never returns 0 (mapping stays well-defined).
 */
export function canonicalDuration(
  signal: ActivitySignalDto,
  thumbs: ThumbDto[],
): number {
  const lastEvent = thumbs.length > 0 ? thumbs[thumbs.length - 1].t : 0;
  const basis = lastEvent > 0 ? lastEvent : signalDuration(signal);
  return basis > 0 ? basis : Math.max(signal.bucket_secs, MIN_VIEW_SECS);
}

/** Map a source time to a pixel X within a `width`-px canvas for `view`. */
export function timeToX(t: number, view: Viewport, width: number): number {
  const span = view.end - view.start;
  if (span <= 0) return 0;
  return ((t - view.start) / span) * width;
}

/** Map a pixel X (0..width) back to a source time within `view`. */
export function xToTime(x: number, view: Viewport, width: number): number {
  if (width <= 0) return view.start;
  return view.start + (x / width) * (view.end - view.start);
}

/** Clamp a span into `[MIN_VIEW_SECS, max(duration, MIN_VIEW_SECS)]`. */
function clampSpan(span: number, duration: number): number {
  const max = Math.max(duration, MIN_VIEW_SECS);
  return Math.min(Math.max(span, MIN_VIEW_SECS), max);
}

/**
 * Slide/resize a viewport back inside `[0, duration]` while preserving its span.
 * A span wider than the duration collapses to the full `[0, duration]` window.
 */
export function clampPan(view: Viewport, duration: number): Viewport {
  const span = clampSpan(view.end - view.start, duration);
  let start = view.start;
  if (start + span > duration) start = duration - span;
  if (start < 0) start = 0;
  return { start, end: start + span };
}

/**
 * Zoom `view` around a focus time by `factor` (`<1` zooms in, `>1` zooms out),
 * keeping `focus` under the same pixel. Result is clamped to `[0, duration]` and
 * to a minimum span, so repeated zooming never inverts or escapes the timeline.
 */
export function zoomAround(
  view: Viewport,
  focus: number,
  factor: number,
  duration: number,
): Viewport {
  const span = view.end - view.start;
  const newSpan = clampSpan(span * factor, duration);
  const frac = span > 0 ? (focus - view.start) / span : 0.5;
  const start = focus - frac * newSpan;
  return clampPan({ start, end: start + newSpan }, duration);
}

/** Translate the viewport by `dtSeconds` (drag-pan), clamped to `[0, duration]`. */
export function panBy(
  view: Viewport,
  dtSeconds: number,
  duration: number,
): Viewport {
  return clampPan(
    { start: view.start + dtSeconds, end: view.end + dtSeconds },
    duration,
  );
}

/**
 * Aggregate the buckets visible in `view` into `columns` per-column MAX scores,
 * for drawing a faithful envelope on a canvas `columns` px wide. The 17-min
 * sample yields thousands of buckets against an ~800 px canvas, so each column
 * takes the max of the buckets it covers rather than dropping them. Returns an
 * array of length `columns` (0 where a column covers no bucket).
 */
export function aggregateColumns(
  signal: ActivitySignalDto,
  view: Viewport,
  columns: number,
): number[] {
  const cols = Math.max(0, Math.floor(columns));
  const out = new Array<number>(cols).fill(0);
  const span = view.end - view.start;
  if (cols === 0 || signal.buckets.length === 0 || span <= 0) return out;

  const { bucket_secs, buckets } = signal;
  for (let i = 0; i < buckets.length; i++) {
    const bucketStart = i * bucket_secs;
    // Skip buckets wholly outside the viewport.
    if (bucketStart + bucket_secs < view.start || bucketStart > view.end) {
      continue;
    }
    const col = Math.floor(((bucketStart - view.start) / span) * cols);
    if (col < 0 || col >= cols) continue;
    if (buckets[i] > out[col]) out[col] = buckets[i];
  }
  return out;
}

/** The maximum bucket score in the signal, for normalizing the envelope height. */
export function peakScore(signal: ActivitySignalDto): number {
  let max = 0;
  for (const b of signal.buckets) if (b > max) max = b;
  return max;
}
