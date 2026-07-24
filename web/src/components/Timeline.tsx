import type { Component } from "solid-js";
import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import type { ActivitySignalDto } from "../lib/api";
import {
  aggregateColumns,
  panBy,
  peakScore,
  timeToX,
  xToTime,
  zoomAround,
  type Viewport,
} from "../lib/timeline";

/** Amber/cyan cutting-room palette (mirrors prototype/ux tokens). */
const KEEP = "#ffb454";
const KEEP_DEEP = "#c8791f";
const DEAD = "#3a464c";
const AXIS = "#20282c";
const PLAYHEAD = "#5ad1e6";

/** Fixed canvas height in CSS pixels. Kept in sync with `.timeline__canvas`
 * in styles.css. Deliberately compact so the activity lane doesn't starve the
 * asciinema preview above it (the timeline row is natural-height). */
const HEIGHT = 84;

/**
 * Pixel movement below which a pointer gesture is a CLICK (seek), not a DRAG
 * (pan). A click is a 0-px drag, so without a threshold every seek would also
 * nudge the pan — this disambiguates the two.
 */
const DRAG_THRESHOLD_PX = 4;

interface TimelineProps {
  /** The change-density waveform to render. */
  signal: ActivitySignalDto;
  /** The visible time window (owned by the Editor view). */
  view: Viewport;
  /** Commit a new viewport (zoom/pan). */
  setView: (view: Viewport) => void;
  /** The playhead time in seconds (owned by the Editor view). */
  playhead: number;
  /** Move the playhead (click-to-seek). */
  setPlayhead: (t: number) => void;
  /** The canonical total duration, for clamping zoom/pan. */
  duration: number;
  /** Source-time kept-segment windows `[start, end]` for amber bar coloring. */
  keepWindows?: readonly (readonly [number, number])[];
  /**
   * Report the source time under the pointer while it is over the lane, and
   * `null` once it leaves. Drives the Editor's hover guide + readout chip
   * (brief §3H): the affordance the shipped click-to-seek never had.
   */
  onHover?: (t: number | null) => void;
}

/**
 * The activity-timeline canvas (AC#1): draws the aggregated change-density
 * waveform for the current viewport, a vertical playhead, and a baseline axis.
 *
 * Interaction: mouse wheel zooms around the cursor time, drag pans (both clamped
 * to the full duration), a click seeks the playhead.
 *
 * ─── DRAG MEANS PAN (brief conflict §4.1, resolved) ──────────────────────────
 * The v2 mock has no zoom and uses drag-to-scrub; this timeline zooms and pans
 * for real, and losing that is as much a regression as losing reorder. So drag
 * stays PAN and source inspection is click + hover: {@link TimelineProps.onHover}
 * reports the time under the pointer so the Editor can draw a guide and a
 * kept/dead/unkept readout chip, and the `crosshair` cursor plus the pane
 * header state the affordance. That gives §3H's "scanning tool" without
 * re-homing a tested gesture onto a modifier that behaves inconsistently across
 * WKWebView / WebView2 / WebKitGTK.
 * ─────────────────────────────────────────────────────────────────────────────
 *
 * The backing store is scaled
 * by `devicePixelRatio` and re-sized on container resize (`ResizeObserver`) so
 * the waveform stays crisp on HiDPI and never renders stale after a resize. A
 * Solid `createEffect` redraws on any viewport / playhead / signal / width
 * change — no manual RAF loop.
 */
export const Timeline: Component<TimelineProps> = (props) => {
  let canvas!: HTMLCanvasElement;
  let container!: HTMLDivElement;
  const [cssWidth, setCssWidth] = createSignal(800);

  // Pointer-gesture state (plain locals — not reactive).
  let dragging = false;
  let moved = false;
  let downX = 0;
  let lastX = 0;

  function draw(): void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = cssWidth();
    const h = HEIGHT;

    // Size the backing store in device pixels; draw in CSS pixels.
    const bw = Math.max(1, Math.floor(w * dpr));
    const bh = Math.max(1, Math.floor(h * dpr));
    if (canvas.width !== bw) canvas.width = bw;
    if (canvas.height !== bh) canvas.height = bh;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.clearRect(0, 0, w, h);
    // Inset background (the wave sits in --inset; the wrap supplies the frame).
    ctx.fillStyle = "#080b0d";
    ctx.fillRect(0, 0, w, h);

    const view = props.view;
    const cols = Math.max(1, Math.floor(w));
    const env = aggregateColumns(props.signal, view, cols);
    const peak = peakScore(props.signal) || 1;
    const keep = props.keepWindows ?? [];
    const inKeep = (t: number): boolean =>
      keep.some(([a, b]) => t >= a && t <= b);

    // Change-density envelope: one column per pixel, amber inside kept segments,
    // faint dead-air otherwise — mirroring the prototype's `.bar.keep` styling.
    const usable = h - 6;
    const span = view.end - view.start;
    for (let x = 0; x < cols; x++) {
      const barH = (env[x] / peak) * usable;
      if (barH <= 0) continue;
      const t = span > 0 ? view.start + (x / cols) * span : 0;
      ctx.fillStyle = inKeep(t) ? KEEP : DEAD;
      ctx.fillRect(x, h - barH, 1, barH);
    }
    // Amber deep base line for kept regions (a subtle glow under the bars).
    ctx.strokeStyle = KEEP_DEEP;
    ctx.lineWidth = 1;
    for (const [a, b] of keep) {
      const xa = timeToX(a, view, w);
      const xb = timeToX(b, view, w);
      if (xb < 0 || xa > w) continue;
      ctx.beginPath();
      ctx.moveTo(Math.max(0, xa), h - 0.5);
      ctx.lineTo(Math.min(w, xb), h - 0.5);
      ctx.stroke();
    }

    // Baseline axis.
    ctx.strokeStyle = AXIS;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, h - 0.5);
    ctx.lineTo(w, h - 0.5);
    ctx.stroke();

    // Playhead (cyan, with a small caret at the top — prototype `.playhead`).
    const px = timeToX(props.playhead, view, w);
    if (px >= 0 && px <= w) {
      ctx.strokeStyle = PLAYHEAD;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(px, -1);
      ctx.lineTo(px, h);
      ctx.stroke();
      ctx.fillStyle = PLAYHEAD;
      ctx.beginPath();
      ctx.moveTo(px - 3, 0);
      ctx.lineTo(px + 3, 0);
      ctx.lineTo(px, 4);
      ctx.closePath();
      ctx.fill();
    }
  }

  function onWheel(e: WheelEvent): void {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const focus = xToTime(e.clientX - rect.left, props.view, rect.width);
    // Wheel down (deltaY > 0) zooms OUT; wheel up zooms IN.
    const factor = e.deltaY > 0 ? 1.1 : 1 / 1.1;
    props.setView(zoomAround(props.view, focus, factor, props.duration));
  }

  function onPointerDown(e: PointerEvent): void {
    dragging = true;
    moved = false;
    downX = e.clientX;
    lastX = e.clientX;
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch {
      // Best-effort: gesture works without capture (synthetic events / edge cases).
    }
  }

  function onPointerMove(e: PointerEvent): void {
    // Report the hovered time whether or not a gesture is in flight, so the
    // guide keeps tracking the pointer during a pan.
    const hoverRect = canvas.getBoundingClientRect();
    props.onHover?.(
      xToTime(e.clientX - hoverRect.left, props.view, hoverRect.width),
    );
    if (!dragging) return;
    if (Math.abs(e.clientX - downX) > DRAG_THRESHOLD_PX) moved = true;
    if (!moved) return;
    const rect = canvas.getBoundingClientRect();
    const span = props.view.end - props.view.start;
    // Drag right → the content moves right → the viewport moves LEFT.
    const dt = -((e.clientX - lastX) / rect.width) * span;
    props.setView(panBy(props.view, dt, props.duration));
    lastX = e.clientX;
  }

  function onPointerUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    try {
      canvas.releasePointerCapture(e.pointerId);
    } catch {
      // Best-effort: release is optional.
    }
    if (moved) return;
    // A click (sub-threshold gesture) seeks the playhead.
    const rect = canvas.getBoundingClientRect();
    props.setPlayhead(xToTime(e.clientX - rect.left, props.view, rect.width));
  }

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setCssWidth(Math.max(1, Math.floor(entry.contentRect.width)));
      }
    });
    ro.observe(container);
    // Wheel must be a non-passive listener to allow preventDefault().
    canvas.addEventListener("wheel", onWheel, { passive: false });
    onCleanup(() => {
      ro.disconnect();
      canvas.removeEventListener("wheel", onWheel);
    });
  });

  // Fine-grained redraw: re-runs whenever signal, view, playhead, or width change.
  createEffect(() => {
    // Touch the reactive inputs so the effect tracks them.
    void props.signal;
    void props.view;
    void props.playhead;
    void cssWidth();
    draw();
  });

  return (
    <div class="timeline" data-testid="timeline" ref={container}>
      <canvas
        ref={canvas}
        class="timeline__canvas"
        style={{ height: `${HEIGHT}px` }}
        role="img"
        aria-label="activity timeline waveform"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={() => props.onHover?.(null)}
      />
    </div>
  );
};
