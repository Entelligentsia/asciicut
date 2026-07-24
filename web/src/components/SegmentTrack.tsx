import type { Component } from "solid-js";
import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { timeToX, xToTime, type Viewport } from "../lib/timeline";
import {
  createSegment,
  hitTest,
  moveWindow,
  remove,
  resizeEdge,
  type EditModel,
  type HitRegion,
} from "../lib/segments";

/** Amber/cyan cutting-room palette (mirrors prototype/ux tokens). */
const BAND = "#ffb454";
const BAND_DEEP = "#c8791f";
const GRIP_IN = "#7fd97f";
const GRIP_OUT = "#ff6b6b";
const GLOW = "rgba(255,180,84,.28)";

/** Fixed canvas height in CSS pixels. Kept in sync with `.segment-track__canvas`
 * in styles.css. The cuts lane is the primary editing surface, so it is given
 * more presence than the reference filmstrip below it. */
const HEIGHT = 58;

/** Pixel width of an edge grab handle (also the resize hit tolerance). */
const EDGE_PX = 6;

/**
 * Pixel movement below which a pointer gesture is a CLICK (select), not a DRAG
 * (draw/move/resize). Mirrors `Timeline.tsx`'s threshold so a select never
 * accidentally nudges a window.
 */
const DRAG_THRESHOLD_PX = 4;

/** What the current pointer gesture is doing. */
type GestureMode = "none" | "draw" | "move" | "resize";

interface SegmentTrackProps {
  /** The immutable segment model (source of truth). */
  model: EditModel;
  /** The SAME visible window the waveform uses, for time-aligned placement. */
  view: Viewport;
  /** The canonical total duration, for clamping draw/drag/resize. */
  duration: number;
  /** The selected segment id, or `null` (owned by the Editor view). */
  selectedId: string | null;
  /** Change the selection. */
  onSelect: (id: string | null) => void;
  /** Commit a new model (every edit routes through a `lib/segments` reducer). */
  onModel: (next: EditModel) => void;
}

/**
 * The segment-track canvas (AC#1): draws each keep-segment as a rectangle with
 * edge grab handles over the SAME viewport the waveform uses, and routes a
 * pointer-gesture state machine to the pure `lib/segments` reducers:
 *   • pointerdown on EMPTY space → DRAW a new window (drag out its span);
 *   • pointerdown on a segment BODY → DRAG the whole window along source time;
 *   • pointerdown on an edge HANDLE → RESIZE that edge;
 *   • a sub-threshold click SELECTS (or clears on empty).
 * `Delete`/`Backspace` removes the selected segment (canvas-side delete). The
 * component is a PURE controlled view: it holds no authoritative state, reads
 * `model`/`view`/`selectedId`, and emits every change via callback props. HiDPI
 * backing-store scaling + `ResizeObserver` + a `createEffect` redraw follow the
 * T09 canvas conventions.
 */
export const SegmentTrack: Component<SegmentTrackProps> = (props) => {
  let canvas!: HTMLCanvasElement;
  let container!: HTMLDivElement;
  const [cssWidth, setCssWidth] = createSignal(800);
  // A live draft rectangle while drawing, in source seconds (redraw input).
  const [draft, setDraft] = createSignal<{ a: number; b: number } | null>(null);

  // Pointer-gesture state (plain locals — not reactive).
  let mode: GestureMode = "none";
  let activeId: string | null = null;
  let resizeEdgeSide: "left" | "right" = "left";
  let downX = 0;
  let lastTime = 0;
  let moved = false;

  /** Edge grab tolerance in source seconds for the current view/width. */
  function edgeTolSecs(): number {
    const span = props.view.end - props.view.start;
    const w = cssWidth();
    return w > 0 ? (EDGE_PX / w) * span : 0;
  }

  function draw(): void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = cssWidth();
    const h = HEIGHT;

    const bw = Math.max(1, Math.floor(w * dpr));
    const bh = Math.max(1, Math.floor(h * dpr));
    if (canvas.width !== bw) canvas.width = bw;
    if (canvas.height !== bh) canvas.height = bh;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.clearRect(0, 0, w, h);
    // Transparent: this canvas overlays the waveform, so never fill a background.

    const view = props.view;
    const top = 6;
    const boxH = h - 12;

    props.model.segments.forEach((seg, i) => {
      const x0 = timeToX(seg.srcStart, view, w);
      const x1 = timeToX(seg.srcEnd, view, w);
      // Skip windows wholly outside the viewport.
      if (x1 < 0 || x0 > w) return;
      const left = Math.max(0, x0);
      const width = Math.max(1, Math.min(w, x1) - left);
      const selected = seg.id === props.selectedId;

      // Band fill (translucent amber; selected = stronger + glow ring).
      ctx.fillStyle = selected
        ? "rgba(255,180,84,0.16)"
        : "rgba(255,180,84,0.06)";
      ctx.fillRect(left, top, width, boxH);
      ctx.strokeStyle = selected ? BAND : BAND_DEEP;
      ctx.lineWidth = selected ? 2 : 1;
      if (selected) {
        ctx.save();
        ctx.shadowColor = GLOW;
        ctx.shadowBlur = 8;
      }
      ctx.strokeRect(left + 0.5, top + 0.5, width - 1, boxH - 1);
      if (selected) ctx.restore();

      // Edge grab handles (green in / red out) — prototype `.band .grip`.
      if (x0 >= 0 && x0 <= w) {
        ctx.fillStyle = GRIP_IN;
        ctx.fillRect(x0 - 1, top, 2, boxH);
      }
      if (x1 >= 0 && x1 <= w) {
        ctx.fillStyle = GRIP_OUT;
        ctx.fillRect(x1 - 1, top, 2, boxH);
      }

      // Tab label: S# · speed× [⏸hold] (prototype `.band .tab`).
      if (width > 28) {
        const label = `S${i + 1} · ${seg.speed.toFixed(1)}×${
          seg.holdEnd > 0 ? ` ⏸${seg.holdEnd.toFixed(1)}s` : ""
        }`;
        ctx.font = "9px 'JetBrains Mono', ui-monospace, monospace";
        ctx.textBaseline = "top";
        const tw = ctx.measureText(label).width;
        // Pill background for legibility over the waveform.
        ctx.fillStyle = "rgba(10,13,15,0.7)";
        ctx.fillRect(left + 4, top + 4, tw + 8, 13);
        ctx.fillStyle = BAND;
        ctx.fillText(label, left + 8, top + 6);
      }
    });

    // Live draft rectangle while drawing.
    const d = draft();
    if (d) {
      const dx0 = timeToX(Math.min(d.a, d.b), view, w);
      const dx1 = timeToX(Math.max(d.a, d.b), view, w);
      ctx.fillStyle = "rgba(255,180,84,0.20)";
      ctx.fillRect(dx0, top, Math.max(1, dx1 - dx0), boxH);
      ctx.strokeStyle = BAND;
      ctx.lineWidth = 1;
      ctx.strokeRect(dx0 + 0.5, top + 0.5, Math.max(1, dx1 - dx0) - 1, boxH - 1);
    }

    // (No baseline axis — the overlay is transparent; the waveform canvas
    // below owns the axis.)
  }

  function pointerTime(e: PointerEvent): number {
    const rect = canvas.getBoundingClientRect();
    return xToTime(e.clientX - rect.left, props.view, rect.width);
  }

  function onPointerDown(e: PointerEvent): void {
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch {
      // setPointerCapture is an enhancement (keeps events flowing when the pointer
      // leaves the canvas); the gesture state machine works without it. Synthetic
      // pointer events have no active pointer, and some edge cases throw — never
      // let that abort the gesture.
    }
    downX = e.clientX;
    moved = false;
    const t = pointerTime(e);
    lastTime = t;
    const hit = hitTest(props.model, t, edgeTolSecs());
    if (hit) {
      activeId = hit.id;
      props.onSelect(hit.id);
      if (hit.region === "body") {
        mode = "move";
      } else {
        mode = "resize";
        resizeEdgeSide = hit.region as Exclude<HitRegion, "body">;
      }
    } else {
      // Empty space → begin drawing a new window from here.
      mode = "draw";
      activeId = null;
      setDraft({ a: t, b: t });
    }
  }

  function onPointerMove(e: PointerEvent): void {
    if (mode === "none") return;
    if (Math.abs(e.clientX - downX) > DRAG_THRESHOLD_PX) moved = true;
    const t = pointerTime(e);
    if (mode === "draw") {
      const d = draft();
      if (d) setDraft({ a: d.a, b: t });
    } else if (mode === "move" && activeId) {
      if (!moved) return;
      props.onModel(moveWindow(props.model, activeId, t - lastTime, props.duration));
      lastTime = t;
    } else if (mode === "resize" && activeId) {
      props.onModel(
        resizeEdge(props.model, activeId, resizeEdgeSide, t, props.duration),
      );
    }
  }

  function onPointerUp(e: PointerEvent): void {
    if (mode === "none") return;
    try {
      canvas.releasePointerCapture(e.pointerId);
    } catch {
      // See onPointerDown: release is best-effort.
    }
    const finishedMode = mode;
    mode = "none";
    const t = pointerTime(e);

    if (finishedMode === "draw") {
      const d = draft();
      setDraft(null);
      if (d && moved) {
        const { model, id } = createSegment(props.model, d.a, t, props.duration);
        props.onModel(model);
        props.onSelect(id);
      } else {
        // A sub-threshold click on empty space clears the selection.
        props.onSelect(null);
      }
    }
    activeId = null;
  }

  function onKeyDown(e: KeyboardEvent): void {
    if ((e.key === "Delete" || e.key === "Backspace") && props.selectedId) {
      e.preventDefault();
      props.onModel(remove(props.model, props.selectedId));
      props.onSelect(null);
    }
  }

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setCssWidth(Math.max(1, Math.floor(entry.contentRect.width)));
      }
    });
    ro.observe(container);
    onCleanup(() => ro.disconnect());
  });

  // Fine-grained redraw: re-runs on model / view / selection / draft / width.
  createEffect(() => {
    void props.model;
    void props.view;
    void props.selectedId;
    void draft();
    void cssWidth();
    draw();
  });

  return (
    <div class="segment-track" data-testid="segment-track" ref={container}>
      <canvas
        ref={canvas}
        class="segment-track__canvas"
        style={{ height: `${HEIGHT}px` }}
        role="img"
        aria-label="editor segment track"
        tabindex="0"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onKeyDown={onKeyDown}
      />
    </div>
  );
};
