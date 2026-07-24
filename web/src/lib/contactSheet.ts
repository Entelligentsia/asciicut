// Pure layout for the per-segment contact sheet (brief §3F/§3G).
//
// The sheet has two axis meanings and one strict rule: cards must never
// overlap. In SOURCE mode x means *time*, so each card wants to sit under its
// own band and is only displaced far enough to stop colliding — a leader line
// then carries the association back to the band's true position. In CUT ORDER
// mode x means *sequence*, so cards simply pack in playback order.
//
// This module is DELIBERATELY dependency-free and side-effect-free: no DOM, no
// canvas, no Solid. All time↔pixel mapping goes through `lib/timeline`'s
// `Viewport` (conflict §4.2 — the real timeline zooms, the mock does not), and
// the placement arithmetic is inspectable in isolation from the components that
// render it.

import { timeToX, type Viewport } from "./timeline";

/** A segment as the sheet needs to see it: an identity and a source window. */
export interface SheetItem {
  /** The segment's stable client id. */
  readonly id: string;
  /** Window start in source seconds. */
  readonly srcStart: number;
  /** Window end in source seconds. */
  readonly srcEnd: number;
}

/** Where one card landed, and where its band actually is. */
export interface SheetPlacement {
  /** The segment's stable client id. */
  readonly id: string;
  /** The card's left edge, in px from the strip's left edge. */
  readonly leftPx: number;
  /**
   * The x the card *wanted* — the band's own centre in the same px space.
   * The leader line runs from here to the card; when it equals the card's
   * centre the card sits exactly under its band and the line is vertical.
   */
  readonly anchorX: number;
}

/** Clamp `v` into `[lo, hi]`, tolerating an inverted range. */
function clamp(v: number, lo: number, hi: number): number {
  if (hi < lo) return lo;
  return v < lo ? lo : v > hi ? hi : v;
}

/**
 * The subset of `items` whose source window is at least partly inside `view`.
 *
 * Conflict §4.2 requires a defined behaviour for segments outside the current
 * viewport. Clamping them to the strip's edge would be a lie — several
 * off-screen segments would pile up on one edge and then be pushed *into* the
 * strip by collision avoidance, implying they are in view. Omitting them keeps
 * the sheet an honest projection of the visible window; the caller reports the
 * count of what it dropped rather than silently truncating.
 */
export function visibleItems(
  items: readonly SheetItem[],
  view: Viewport,
): SheetItem[] {
  return items.filter(
    (item) => item.srcEnd >= view.start && item.srcStart <= view.end,
  );
}

/**
 * SOURCE mode: anchor each card under its own band, then resolve collisions.
 *
 * Two passes, mirroring the mock: left→right shoves any card that overlaps its
 * predecessor to the right, then — if that pushed the last card past the right
 * edge — right→left pulls the whole run back inside. The result is monotonic in
 * time (card order always matches band order) and always fully on-strip.
 *
 * `items` need not be sorted; the returned placements are ordered by anchor.
 * A strip narrower than a single card yields cards pinned at 0 rather than
 * negative positions.
 */
export function layoutSourceMode(
  items: readonly SheetItem[],
  view: Viewport,
  width: number,
  cardWidth: number,
  gap: number,
): SheetPlacement[] {
  if (items.length === 0 || width <= 0) return [];
  const half = cardWidth / 2;
  // A strip too narrow for one card has no room to resolve anything.
  const lo = Math.min(half, width / 2);
  const hi = Math.max(width - half, lo);

  const placed = items
    .map((item) => {
      const mid = (item.srcStart + item.srcEnd) / 2;
      const anchorX = timeToX(mid, view, width);
      return { id: item.id, anchorX, x: clamp(anchorX, lo, hi) };
    })
    .sort((a, b) => a.anchorX - b.anchorX);

  // Pass 1 — left→right: never let a card start before its predecessor ends.
  for (let i = 1; i < placed.length; i++) {
    const minX = placed[i - 1].x + cardWidth + gap;
    if (placed[i].x < minX) placed[i].x = minX;
  }
  // Pass 2 — right→left: if pass 1 overflowed, walk the run back inside.
  const last = placed[placed.length - 1];
  if (last.x > hi) {
    last.x = hi;
    for (let i = placed.length - 2; i >= 0; i--) {
      const maxX = placed[i + 1].x - cardWidth - gap;
      if (placed[i].x > maxX) placed[i].x = maxX;
    }
  }

  return placed.map((p) => ({
    id: p.id,
    leftPx: p.x - half,
    anchorX: p.anchorX,
  }));
}

/**
 * CUT ORDER mode: pack `count` cards left-to-right in playback order, centred
 * in the strip. x means sequence here, so there is nothing to anchor to and no
 * leader lines — `anchorX` is the card's own centre.
 *
 * A run wider than the strip starts at 0 and overflows to the right, which the
 * caller renders inside a horizontally scrollable strip rather than shrinking
 * cards below legibility.
 */
export function layoutCutOrder(
  ids: readonly string[],
  width: number,
  cardWidth: number,
  gap: number,
): SheetPlacement[] {
  if (ids.length === 0) return [];
  const total = ids.length * cardWidth + (ids.length - 1) * gap;
  const x0 = Math.max(0, (width - total) / 2);
  return ids.map((id, i) => {
    const leftPx = x0 + i * (cardWidth + gap);
    return { id, leftPx, anchorX: leftPx + cardWidth / 2 };
  });
}

/**
 * Which slot a dragged card's centre is currently over, for the live-swap
 * re-sequence gesture. Pure so the drag handler stays a thin event adapter.
 */
export function slotAt(
  centreX: number,
  count: number,
  width: number,
  cardWidth: number,
  gap: number,
): number {
  if (count <= 0) return 0;
  const total = count * cardWidth + (count - 1) * gap;
  const x0 = Math.max(0, (width - total) / 2);
  const slot = Math.round((centreX - x0 - cardWidth / 2) / (cardWidth + gap));
  return clamp(slot, 0, count - 1);
}
