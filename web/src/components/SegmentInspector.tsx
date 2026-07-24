import type { Component } from "solid-js";
import { createSignal, Show } from "solid-js";
import {
  remove,
  resizeEdge,
  segmentTag,
  setHoldEnd,
  setSpeed,
  type EditModel,
  type EditSegment,
} from "../lib/segments";
import { scheduleOf } from "../lib/schedule";
import { snapBy } from "../lib/events";
import { mmss, mmssx } from "../lib/format";

/** Speed stepper bounds — mirrors the shipped stepper, unchanged. */
const SPEED_STEP = 0.5;
const SPEED_MAX = 4;
const SPEED_MIN = 0.5;
/** Hold stepper bounds — mirrors the shipped stepper, unchanged. */
const HOLD_STEP = 0.5;
const HOLD_MAX = 5;

/** The hint shown when nothing is hovered — the slot is never empty. */
const DEFAULT_HINT = "Hover a control to see what it changes.";

interface SegmentInspectorProps {
  /** The immutable segment model (source of truth). */
  model: EditModel;
  /** The selected segment id, or `null`. */
  selectedId: string | null;
  /** Commit a new model (every edit routes through a `lib/segments` reducer). */
  onModel: (next: EditModel) => void;
  /** The canonical source duration, for clamping an OUT nudge. */
  duration: number;
  /** The cast's real event times — the grid IN/OUT nudges snap to. */
  eventTimes: readonly number[];
  /** Whether this segment is currently soloed. */
  soloing: boolean;
  /** Toggle solo playback of the selected segment. */
  onToggleSolo: () => void;
  /** Whether solo playback loops. */
  looping: boolean;
  /** Toggle the loop flag. */
  onToggleLoop: () => void;
  /** Whether teaching copy is enabled (the footer `hints on|off` toggle). */
  hintsOn: boolean;
  /**
   * A boundary was just nudged: preview that exact frame. Carries the source
   * time and which mark moved, so the Editor can seek and badge the terminal.
   */
  onNudged: (which: "in" | "out", t: number) => void;
}

/**
 * The segment inspector (brief §3C/§3D/§3E).
 *
 * Replaces three stacked bordered stepper blocks and three permanently-visible
 * hint paragraphs with a compact panel whose teaching happens through a
 * *diagram* instead of prose:
 *
 *   • a **per-segment ratio line** (`28s ──▶ 6.0s on screen`) echoing the
 *     header's grammar at segment scale;
 *   • a **window bar** — solid = playback time, hatched = end hold — that
 *     visibly changes as speed and hold are adjusted, replacing the two
 *     sentences that used to describe them;
 *   • **IN/OUT nudge steppers** that snap to real event timestamps
 *     (`lib/events`), so a boundary always lands on a frame where something
 *     changed, and preview that exact boundary frame as they move;
 *   • **speed / hold as compact label-left rows**, not full-width sections;
 *   • **one contextual hint slot**, fixed height, pinned to the bottom, showing
 *     the hint for the hovered or focused control only.
 *
 * The hint slot is `min-height`-fixed and `margin-top:auto`-pinned so hovering
 * a control can never reflow the panel (§3C's AC). `← prev` / `next →` are gone
 * — `[` / `]` and the contact sheet already navigate — and `↻ loop` is an icon
 * toggle beside the solo button rather than a labelled row.
 *
 * Like the component it replaces, this is a PURE PROJECTION of the model: it
 * holds no authoritative state beyond which hint is showing, and every edit
 * routes through a `lib/segments` reducer. The `segment-*` `data-testid` seams
 * are preserved.
 */
export const SegmentInspector: Component<SegmentInspectorProps> = (props) => {
  const [hint, setHint] = createSignal<string | null>(null);

  const selected = (): EditSegment | undefined =>
    props.model.segments.find((s) => s.id === props.selectedId);

  /** The selected segment's approximate on-screen duration (cut timeline). */
  const cutDurOf = (seg: EditSegment): number =>
    scheduleOf(props.model).segments.find((s) => s.id === seg.id)?.cutDur ?? 0;

  const stepSpeed = (delta: number): void => {
    const s = selected();
    if (!s) return;
    const v = Math.max(
      SPEED_MIN,
      Math.min(SPEED_MAX, +(s.speed + delta).toFixed(1)),
    );
    props.onModel(setSpeed(props.model, s.id, v));
  };

  const stepHold = (delta: number): void => {
    const s = selected();
    if (!s) return;
    const v = Math.max(0, Math.min(HOLD_MAX, +(s.holdEnd + delta).toFixed(1)));
    props.onModel(setHoldEnd(props.model, s.id, v));
  };

  /**
   * Nudge a boundary onto the next/previous real event. `resizeEdge` still owns
   * the clamping invariants (min width, `[0, duration]`), so a snap that would
   * invert or collapse the window is absorbed there rather than special-cased
   * here — the stepper cannot produce an illegal model.
   */
  const nudge = (which: "in" | "out", steps: number): void => {
    const s = selected();
    if (!s) return;
    const from = which === "in" ? s.srcStart : s.srcEnd;
    const target = snapBy(props.eventTimes, from, steps);
    if (target === from) return;
    const next = resizeEdge(
      props.model,
      s.id,
      which === "in" ? "left" : "right",
      target,
      props.duration,
    );
    props.onModel(next);
    // Report the boundary that actually landed, post-clamp, not the request.
    const landed = next.segments.find((x) => x.id === s.id);
    if (landed) {
      props.onNudged(which, which === "in" ? landed.srcStart : landed.srcEnd);
    }
  };

  const drop = (): void => {
    const s = selected();
    if (!s) return;
    props.onModel(remove(props.model, s.id));
  };

  /**
   * Wire a control into the single hint slot. Hover AND focus both show it, so
   * the teaching is reachable by keyboard, not just by mouse.
   */
  const hinted = (
    text: string,
  ): {
    onMouseEnter: () => void;
    onMouseLeave: () => void;
    onFocusIn: () => void;
    onFocusOut: () => void;
  } => ({
    onMouseEnter: () => setHint(text),
    onMouseLeave: () => setHint(null),
    onFocusIn: () => setHint(text),
    onFocusOut: () => setHint(null),
  });

  return (
    <div class="segment-controls insp" data-testid="segment-controls">
      <div class="pane-h">
        <span class="k u amber">◆ segment</span>
        <span class="k mut" data-testid="segment-controls-count">
          {props.model.segments.length} kept
        </span>
        <span class="rule" />
      </div>

      <Show
        when={selected()}
        keyed
        fallback={
          <div class="insp__empty" data-testid="segment-noselect">
            <img
              class="insp__mascot"
              src="/assets/ui_empty_state_transparent.png"
              alt=""
            />
            <span class="big">Nothing selected</span>
            Pick a segment on the <b class="amber">cuts</b> lane or in the
            contact sheet — or drag on the lane to cut a new one.
          </div>
        }
      >
        {(seg) => {
          const window = (): number => Math.max(0, seg.srcEnd - seg.srcStart);
          const onScreen = (): number => cutDurOf(seg);
          const play = (): number =>
            Math.max(0, onScreen() - Math.max(0, seg.holdEnd));
          const hold = (): number => Math.max(0, seg.holdEnd);
          const playPct = (): number =>
            onScreen() > 0 ? (play() / onScreen()) * 100 : 100;

          return (
            <div class="insp__body">
              <div class="seg-title">
                <span class="id">{segmentTag(seg)}</span>
                <span class="lab">{seg.label ?? "untitled window"}</span>
              </div>

              {/* Per-segment ratio — the header's grammar at segment scale. */}
              <div
                class="ratio-seg"
                data-testid="segment-ratio"
                {...hinted(
                  `This segment's ${window().toFixed(0)}s of source becomes ${onScreen().toFixed(1)}s on screen.`,
                )}
              >
                <span class="win">{window().toFixed(0)}s</span>
                <span class="arw">──▶</span>
                <span class="out">{onScreen().toFixed(1)}s</span>
                <span class="on">on screen</span>
              </div>

              {/* The live diagram that replaces two sentences of prose. */}
              <div
                class="wbar"
                data-testid="segment-window-bar"
                role="img"
                aria-label={`${play().toFixed(1)} seconds playback, ${hold().toFixed(1)} seconds hold`}
                {...hinted(
                  "Amber is playback, hatched is the end hold. Speed shrinks the first, hold grows the second.",
                )}
              >
                <i class="p" style={{ width: `${playPct()}%` }} />
                <i class="h" style={{ width: `${100 - playPct()}%` }} />
              </div>
              <div class="wlegend">
                <span>{play().toFixed(1)}s play</span>
                <span>{hold() > 0 ? `${hold().toFixed(1)}s hold` : "no hold"}</span>
              </div>

              {/* Solo / loop — play just this segment (brief §3E). */}
              <div class="solo-row">
                <button
                  type="button"
                  class="solo"
                  classList={{ on: props.soloing }}
                  data-testid="segment-solo"
                  aria-pressed={props.soloing}
                  onClick={props.onToggleSolo}
                  {...hinted(
                    "Plays only this segment, so you can judge its boundaries in isolation.",
                  )}
                >
                  {props.soloing ? "❚❚ Stop solo" : "▸ Play segment"}
                </button>
                <button
                  type="button"
                  class="loopb"
                  classList={{ on: props.looping }}
                  data-testid="segment-loop"
                  aria-pressed={props.looping}
                  aria-label="Loop the soloed segment"
                  title="Loop"
                  onClick={props.onToggleLoop}
                  {...hinted(
                    "Repeat the segment while soloing, so edits show up on the next pass.",
                  )}
                >
                  ↻
                </button>
              </div>

              {/* IN / OUT nudge — snaps to real event times (brief §3D). */}
              <div class="marks">
                <div
                  class="mark in"
                  {...hinted(
                    "In-point. Nudging snaps to the next real event, so it always lands on a frame where something changed. Hold Shift for a coarser step.",
                  )}
                >
                  <div class="ml u">
                    in<span class="snap">· snaps to events</span>
                  </div>
                  <div class="row">
                    <button
                      type="button"
                      class="nudge"
                      data-testid="segment-in-dec"
                      aria-label="Nudge in-point earlier"
                      onClick={(e) => nudge("in", e.shiftKey ? -10 : -1)}
                    >
                      −
                    </button>
                    <span class="mv" data-testid="segment-in">
                      {mmssx(seg.srcStart)}
                    </span>
                    <button
                      type="button"
                      class="nudge"
                      data-testid="segment-in-inc"
                      aria-label="Nudge in-point later"
                      onClick={(e) => nudge("in", e.shiftKey ? 10 : 1)}
                    >
                      +
                    </button>
                  </div>
                </div>
                <div
                  class="mark out"
                  {...hinted(
                    "Out-point. Nudging snaps to the next real event, so it always lands on a frame where something changed. Hold Shift for a coarser step.",
                  )}
                >
                  <div class="ml u">
                    out<span class="snap">· snaps to events</span>
                  </div>
                  <div class="row">
                    <button
                      type="button"
                      class="nudge"
                      data-testid="segment-out-dec"
                      aria-label="Nudge out-point earlier"
                      onClick={(e) => nudge("out", e.shiftKey ? -10 : -1)}
                    >
                      −
                    </button>
                    <span class="mv" data-testid="segment-out">
                      {mmssx(seg.srcEnd)}
                    </span>
                    <button
                      type="button"
                      class="nudge"
                      data-testid="segment-out-inc"
                      aria-label="Nudge out-point later"
                      onClick={(e) => nudge("out", e.shiftKey ? 10 : 1)}
                    >
                      +
                    </button>
                  </div>
                </div>
              </div>
              <div class="marks-sub">
                source {mmss(seg.srcStart)}–{mmss(seg.srcEnd)}
              </div>

              {/* Compact label-left rows (brief §3C). */}
              <div
                class="crow"
                {...hinted(
                  "1× stays honest on the payoff · 2×+ compresses drawing and scrolling.",
                )}
              >
                <label class="u">speed</label>
                <div class="cstep">
                  <button
                    type="button"
                    data-testid="segment-speed-dec"
                    aria-label="Slower"
                    onClick={() => stepSpeed(-SPEED_STEP)}
                  >
                    −
                  </button>
                  <b data-testid="segment-speed">{seg.speed.toFixed(1)}×</b>
                  <button
                    type="button"
                    data-testid="segment-speed-inc"
                    aria-label="Faster"
                    onClick={() => stepSpeed(SPEED_STEP)}
                  >
                    +
                  </button>
                </div>
                {/* Hidden numeric mirrors keep the T11 automation contract. */}
                <input type="hidden" value={seg.speed} data-testid="segment-speed-input" />
              </div>

              <div
                class="crow"
                {...hinted(
                  "Freezes the last frame, giving the viewer a beat to read the result before the cut.",
                )}
              >
                <label class="u">hold</label>
                <div class="cstep">
                  <button
                    type="button"
                    data-testid="segment-hold-dec"
                    aria-label="Less hold"
                    onClick={() => stepHold(-HOLD_STEP)}
                  >
                    −
                  </button>
                  <b data-testid="segment-holdend">{seg.holdEnd.toFixed(1)}s</b>
                  <button
                    type="button"
                    data-testid="segment-hold-inc"
                    aria-label="More hold"
                    onClick={() => stepHold(HOLD_STEP)}
                  >
                    +
                  </button>
                </div>
                <input
                  type="hidden"
                  value={seg.holdEnd}
                  data-testid="segment-holdend-input"
                />
                <button
                  type="button"
                  class="btn danger"
                  data-testid="segment-delete"
                  style={{ "margin-left": "auto" }}
                  onClick={drop}
                  {...hinted(
                    "Removes this segment from the cut. The source recording is never touched.",
                  )}
                >
                  ✂ drop
                </button>
              </div>
            </div>
          );
        }}
      </Show>

      {/* One hint at a time, fixed height, pinned to the bottom. */}
      <Show when={props.hintsOn}>
        <div class="hintslot" data-testid="segment-hint" aria-live="polite">
          {hint() ?? DEFAULT_HINT}
        </div>
      </Show>
    </div>
  );
};
