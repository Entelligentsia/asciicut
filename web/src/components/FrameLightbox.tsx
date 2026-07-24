import type { Component } from "solid-js";
import { onCleanup, onMount, Show } from "solid-js";
import type { FrameDto } from "../lib/api";
import { FrameGrid } from "./FrameGrid";
import { segmentTag, type EditSegment } from "../lib/segments";
import { mmss } from "../lib/format";

interface FrameLightboxProps {
  /** The segment whose median frame is on show, or `null` when closed. */
  segment: EditSegment | null;
  /** The frame at that segment's median source time (`null` while loading). */
  frame: FrameDto | null;
  /** The segment's on-screen duration, for the footer summary. */
  cutDurSecs: number;
  /** Dismiss the lightbox. */
  onClose: () => void;
}

/**
 * The contact sheet's `⤢` action: the segment's median frame at full, readable
 * size (brief §3F).
 *
 * The card thumbnail is a ~150px scaled-down text block — enough to recognise a
 * moment, not enough to read it. This renders the SAME frame through the styled
 * {@link FrameGrid} the rest of the app uses, so "enlarging shows the true frame
 * at the median time" is literally the same data, not a second rendering path.
 *
 * `Escape` closes it, as does clicking the backdrop; the key listener is bound
 * only while a segment is showing so it never competes with the editor's own
 * shortcuts.
 */
export const FrameLightbox: Component<FrameLightboxProps> = (props) => {
  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key === "Escape" && props.segment) {
      e.stopPropagation();
      props.onClose();
    }
  };

  onMount(() => window.addEventListener("keydown", onKeyDown, true));
  onCleanup(() => window.removeEventListener("keydown", onKeyDown, true));

  return (
    <Show when={props.segment}>
      {(seg) => (
        <div
          class="lightbox open"
          role="dialog"
          aria-modal="true"
          aria-label={`${segmentTag(seg())} median frame`}
          data-testid="frame-lightbox"
          onClick={(e) => {
            if (e.target === e.currentTarget) props.onClose();
          }}
        >
          <div class="lb-card">
            <div class="lb-h">
              <span class="sid">{segmentTag(seg())}</span>
              <span class="mut">{seg().label ?? "window"}</span>
              <button
                type="button"
                class="x"
                data-testid="frame-lightbox-close"
                aria-label="Close"
                onClick={props.onClose}
              >
                ✕
              </button>
            </div>
            <div class="lb-b">
              <Show
                when={props.frame}
                fallback={<p class="notice">Rendering the median frame…</p>}
              >
                {(frame) => <FrameGrid frame={frame()} />}
              </Show>
            </div>
            <div class="lb-f">
              median frame @{" "}
              <span class="amber">
                {mmss((seg().srcStart + seg().srcEnd) / 2)}
              </span>{" "}
              · source {mmss(seg().srcStart)}–{mmss(seg().srcEnd)} ·{" "}
              {props.cutDurSecs.toFixed(1)}s on screen
            </div>
          </div>
        </div>
      )}
    </Show>
  );
};
