import type { Component } from "solid-js";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { fetchFrame, type FrameDto } from "../lib/api";
import { segmentTag, reorder, type EditModel, type EditSegment } from "../lib/segments";
import { scheduleOf } from "../lib/schedule";
import { mmss } from "../lib/format";
import {
  layoutCutOrder,
  layoutSourceMode,
  slotAt,
  visibleItems,
  type SheetPlacement,
} from "../lib/contactSheet";
import type { Viewport } from "../lib/timeline";

/** Card geometry, shared with the pure layout math. */
const CARD_W = 150;
const CARD_GAP = 7;

/** Which axis the strip's horizontal position means. */
export type StripMode = "source" | "cut";

interface ContactSheetProps {
  /** The segment model (source of truth for windows + playback order). */
  model: EditModel;
  /** The visible source window — every x↔time mapping goes through it (§4.2). */
  view: Viewport;
  /** The canonical source duration. */
  duration: number;
  /** The selected segment id, or `null`. */
  selectedId: string | null;
  /** Select a segment. */
  onSelect: (id: string) => void;
  /** Commit a new model (reorder routes through `lib/segments`). */
  onModel: (next: EditModel) => void;
  /** Solo-play a segment (double-click). */
  onSolo: (id: string) => void;
  /** Open a segment's median frame at full size (the `⤢` action). */
  onEnlarge: (seg: EditSegment, frame: FrameDto | null) => void;
  /** The strip axis mode (owned by the Editor so playback can auto-switch it). */
  mode: StripMode;
  /** Change the strip axis mode (a user toggle; disables auto-management). */
  onMode: (mode: StripMode) => void;
  /** True when the current mode was chosen by playback, not the user. */
  autoSwitched: boolean;
  /** The id of the segment currently playing, highlighted as a progress cue. */
  playingId: string | null;
  /** Whether teaching copy is enabled. */
  hintsOn: boolean;
}

/**
 * The per-segment contact sheet (brief §3F/§3G) — replaces the old filmstrip.
 *
 * The filmstrip sampled 24 frames evenly across the RAW recording and placed
 * them by absolute time; at the sample's scale cells overlapped ~6-deep and
 * were unreadable, and it answered a question ("what's at 8:00 of the source?")
 * that stops mattering once segments exist. This shows ONE thumbnail per
 * segment, rendered at that segment's median source time, and has an explicit
 * axis mode because a contact sheet (source-time) and reorder (cut-order) are
 * two different meanings for one horizontal axis:
 *
 *   • SOURCE mode — each card sits under its own band via `layoutSourceMode`'s
 *     collision avoidance, with an SVG leader line back to the band's true
 *     position (accented for the selection). All mapping goes through the
 *     shared `Viewport`, so cards track zoom/pan; segments scrolled out of view
 *     are dropped (not clamped — see `visibleItems`) and their count reported.
 *   • CUT ORDER mode — cards pack in playback order with order badges, leaders
 *     hidden, and drag / `←`·`→` re-sequence through the `reorder` reducer.
 *
 * ─── PERFORMANCE: median frames are cached per segment (brief §3F) ───────────
 * `frame_at(t)` replays the event stream `0→t`, so rendering N medians naïvely
 * is N full replays on every model change. The cache below is keyed by the
 * segment id AND its median time (rounded to a frame), so a card only re-fetches
 * when THAT segment's bounds move — speed/hold/reorder edits, and edits to other
 * segments, reuse the cached frame. Entries for deleted segments are pruned.
 * ─────────────────────────────────────────────────────────────────────────────
 */
export const ContactSheet: Component<ContactSheetProps> = (props) => {
  let container!: HTMLDivElement;
  const [width, setWidth] = createSignal(800);
  // id → { key, frame } — the median-frame cache (see the performance note).
  const [frames, setFrames] = createSignal<Map<string, { key: string; frame: FrameDto | null }>>(
    new Map(),
  );
  // Drag-to-reorder state (cut mode). Plain locals + a reactive draft order.
  const [dragId, setDragId] = createSignal<string | null>(null);

  const median = (seg: EditSegment): number => (seg.srcStart + seg.srcEnd) / 2;
  /** A stable cache key: same segment, same median-to-a-frame → same frame. */
  const cacheKey = (seg: EditSegment): string =>
    `${seg.id}@${median(seg).toFixed(2)}`;

  // Fetch any median frame not already cached; prune deleted segments. Runs on
  // any model change but only does work for segments whose median actually
  // moved (the key check), so it is O(changed), not O(N), per edit.
  createEffect(() => {
    const segs = props.model.segments;
    const cache = frames();
    const wanted = new Set(segs.map((s) => s.id));
    let mutated = false;
    const next = new Map(cache);
    // Prune.
    for (const id of cache.keys()) {
      if (!wanted.has(id)) {
        next.delete(id);
        mutated = true;
      }
    }
    // Fetch stale / missing.
    for (const seg of segs) {
      const key = cacheKey(seg);
      if (next.get(seg.id)?.key === key) continue;
      const t = median(seg);
      void fetchFrame(t)
        .then((frame) => {
          setFrames((m) => {
            // Guard against a race: only write if the segment still wants this
            // exact median (a rapid drag can outrun the fetch).
            const cur = props.model.segments.find((s) => s.id === seg.id);
            if (!cur || cacheKey(cur) !== key) return m;
            const copy = new Map(m);
            copy.set(seg.id, { key, frame });
            return copy;
          });
        })
        .catch(() => {
          setFrames((m) => {
            const copy = new Map(m);
            copy.set(seg.id, { key, frame: null });
            return copy;
          });
        });
    }
    if (mutated) setFrames(next);
  });

  // Placements for the current mode (memoized on the inputs that move cards).
  const placements = createMemo<SheetPlacement[]>(() => {
    const w = width();
    if (props.mode === "cut") {
      return layoutCutOrder(
        props.model.segments.map((s) => s.id),
        w,
        CARD_W,
        CARD_GAP,
      );
    }
    const items = visibleItems(props.model.segments, props.view);
    return layoutSourceMode(items, props.view, w, CARD_W, CARD_GAP);
  });

  const placementOf = (id: string): SheetPlacement | undefined =>
    placements().find((p) => p.id === id);

  /** How many segments are hidden because they fall outside the viewport. */
  const hiddenCount = createMemo<number>(() =>
    props.mode === "cut"
      ? 0
      : props.model.segments.length -
        visibleItems(props.model.segments, props.view).length,
  );

  const cutDurOf = (seg: EditSegment): number =>
    scheduleOf(props.model).segments.find((s) => s.id === seg.id)?.cutDur ?? 0;

  // ─── Drag-to-reorder (cut mode) ────────────────────────────────────────────
  const onCardPointerDown = (e: PointerEvent, seg: EditSegment): void => {
    if (props.mode !== "cut") return;
    if ((e.target as HTMLElement).closest(".zoom, .reorder")) return;
    const rect = container.getBoundingClientRect();
    const startX = e.clientX;
    let moved = false;
    setDragId(seg.id);
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);

    const onMove = (ev: PointerEvent): void => {
      if (Math.abs(ev.clientX - startX) > 3) moved = true;
      if (!moved) return;
      const centreX = ev.clientX - rect.left;
      const from = props.model.segments.findIndex((s) => s.id === seg.id);
      const to = slotAt(
        centreX,
        props.model.segments.length,
        rect.width,
        CARD_W,
        CARD_GAP,
      );
      if (to !== from && from >= 0) {
        props.onModel(reorder(props.model, from, to));
      }
    };
    const onUp = (ev: PointerEvent): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setDragId(null);
      if (!moved) props.onSelect(seg.id);
      void ev;
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const stepOrder = (seg: EditSegment, dir: -1 | 1): void => {
    const from = props.model.segments.findIndex((s) => s.id === seg.id);
    if (from < 0) return;
    props.onModel(reorder(props.model, from, from + dir));
  };

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setWidth(Math.max(1, Math.floor(entry.contentRect.width)));
      }
    });
    ro.observe(container);
    onCleanup(() => ro.disconnect());
  });

  const caption = (): string =>
    props.mode === "cut"
      ? "playback order · drag a card to re-sequence · ← → on the selected card"
      : "one frame per kept segment, under its own cut · click to select · ⤢ to enlarge";

  return (
    <div class="sheet" data-testid="contact-sheet">
      {/* Leader lines tie each source-mode thumbnail to its band above. */}
      <Show when={props.mode === "source"}>
        <svg class="leaders" aria-hidden="true">
          <For each={placements()}>
            {(p) => {
              const sel = (): boolean => p.id === props.selectedId;
              return (
                <>
                  <line
                    classList={{ sel: sel() }}
                    x1={p.anchorX}
                    y1={0}
                    x2={p.leftPx + CARD_W / 2}
                    y2={15}
                  />
                  <circle classList={{ sel: sel() }} cx={p.anchorX} cy={0} r={1.6} />
                </>
              );
            }}
          </For>
        </svg>
      </Show>

      <div
        class="contact"
        classList={{ cutmode: props.mode === "cut" }}
        data-testid="contact-strip"
        ref={container}
      >
        <Show
          when={props.model.segments.length > 0}
          fallback={
            <div class="contact-empty" data-testid="contact-empty">
              No segments yet — draw one on the cuts lane to begin.
            </div>
          }
        >
          <For each={props.model.segments}>
            {(seg) => {
              const place = (): SheetPlacement | undefined => placementOf(seg.id);
              const pos = (): number =>
                props.model.segments.findIndex((s) => s.id === seg.id);
              const cached = (): FrameDto | null =>
                frames().get(seg.id)?.frame ?? null;
              return (
                <Show when={place()}>
                  {(p) => (
                    <figure
                      class="shot"
                      classList={{
                        sel: seg.id === props.selectedId,
                        soloing: seg.id === props.playingId,
                        playing: seg.id === props.playingId,
                        dragging: seg.id === dragId(),
                      }}
                      data-testid="contact-card"
                      data-seg-id={seg.id}
                      style={{ left: `${p().leftPx}px` }}
                      onPointerDown={(e) => onCardPointerDown(e, seg)}
                      onClick={() => props.onSelect(seg.id)}
                      onDblClick={() => props.onSolo(seg.id)}
                    >
                      <div class="thumb">
                        <span class="playdot" />
                        <Show when={props.mode === "cut"}>
                          <span class="ord">{pos() + 1}</span>
                        </Show>
                        <button
                          type="button"
                          class="zoom"
                          data-testid="contact-enlarge"
                          aria-label={`Enlarge ${segmentTag(seg)} frame`}
                          title={`Enlarge (median frame @ ${mmss(median(seg))})`}
                          onClick={(e) => {
                            e.stopPropagation();
                            props.onEnlarge(seg, cached());
                          }}
                        >
                          ⤢
                        </button>
                        <pre class="mini">
                          {cached() ? cached()!.text.join("\n") : "…"}
                        </pre>
                        <Show when={props.mode === "cut" && seg.id === props.selectedId}>
                          <span class="reorder">
                            <button
                              type="button"
                              data-testid="contact-move-earlier"
                              aria-label="Move earlier in playback order"
                              disabled={pos() === 0}
                              onClick={(e) => {
                                e.stopPropagation();
                                stepOrder(seg, -1);
                              }}
                            >
                              ←
                            </button>
                            <button
                              type="button"
                              data-testid="contact-move-later"
                              aria-label="Move later in playback order"
                              disabled={pos() === props.model.segments.length - 1}
                              onClick={(e) => {
                                e.stopPropagation();
                                stepOrder(seg, 1);
                              }}
                            >
                              →
                            </button>
                          </span>
                        </Show>
                      </div>
                      <figcaption class="meta">
                        <span class="sid">{segmentTag(seg)}</span>
                        <span class="slab">{seg.label ?? "window"}</span>
                        <span class="sdur">{cutDurOf(seg).toFixed(1)}s</span>
                      </figcaption>
                    </figure>
                  )}
                </Show>
              );
            }}
          </For>
        </Show>
      </div>

      {/* Axis legend + mode toggle, directly under the strip. */}
      <div class="capbar">
        <span class="stripmode" data-testid="strip-mode" role="group" aria-label="Strip axis">
          <button
            type="button"
            data-testid="strip-mode-source"
            classList={{ on: props.mode === "source" }}
            aria-pressed={props.mode === "source"}
            onClick={() => props.onMode("source")}
          >
            source
          </button>
          <button
            type="button"
            data-testid="strip-mode-cut"
            classList={{
              on: props.mode === "cut" && !props.autoSwitched,
              auto: props.mode === "cut" && props.autoSwitched,
            }}
            aria-pressed={props.mode === "cut"}
            onClick={() => props.onMode("cut")}
          >
            cut order
          </button>
        </span>
        <Show when={props.hintsOn}>
          <span class="k mut" style={{ "font-size": "10px" }}>
            {caption()}
          </span>
        </Show>
        <Show when={hiddenCount() > 0}>
          <span class="k mut strip-hidden" data-testid="contact-hidden" style={{ "margin-left": "auto", "font-size": "10px" }}>
            {hiddenCount()} outside view — zoom out to see {hiddenCount() === 1 ? "it" : "them"}
          </span>
        </Show>
      </div>
    </div>
  );
};
