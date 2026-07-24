import type { Component } from "solid-js";
import { For, Show } from "solid-js";
import type { CellDto, ColorDto, FrameDto, StyleDto } from "../lib/api";

/** Map a DTO color to a CSS color string. */
function cssColor(color: ColorDto): string {
  if (color.kind === "rgb") {
    return `rgb(${color.r}, ${color.g}, ${color.b})`;
  }
  // Indexed palette: hand the raw index to the browser's ANSI-ish rendering by
  // approximating with a data attribute is overkill here; use a neutral mapping.
  return `var(--ansi-${color.index}, inherit)`;
}

/** Translate a StyleDto into an inline style object for a cell span. */
function cellStyle(style: StyleDto): Record<string, string> {
  const s: Record<string, string> = {};
  if (style.foreground) s.color = cssColor(style.foreground);
  if (style.background) s["background-color"] = cssColor(style.background);
  if (style.bold) s["font-weight"] = "bold";
  if (style.faint) s.opacity = "0.6";
  if (style.italic) s["font-style"] = "italic";
  const decorations: string[] = [];
  if (style.underline) decorations.push("underline");
  if (style.strikethrough) decorations.push("line-through");
  if (decorations.length) s["text-decoration"] = decorations.join(" ");
  if (style.inverse) s.filter = "invert(1)";
  return s;
}

const Cell: Component<{ cell: CellDto }> = (props) => (
  <span class="frame__cell" style={cellStyle(props.cell.style)}>
    {props.cell.ch === " " ? "\u00a0" : props.cell.ch}
  </span>
);

/**
 * Render a server-returned {@link FrameDto} as a styled monospace grid.
 * Row-major over `frame.cells`; a `data-testid` seam exposes it to E2E.
 */
export const FrameGrid: Component<{ frame: FrameDto }> = (props) => (
  <div class="frame" data-testid="frame-grid">
    <Show when={props.frame.marker}>
      <div class="frame__marker" data-testid="frame-marker">
        {props.frame.marker}
      </div>
    </Show>
    <div
      class="frame__grid"
      role="img"
      aria-label={`terminal frame ${props.frame.width}x${props.frame.height}`}
    >
      <For each={props.frame.cells}>
        {(row) => (
          <div class="frame__row">
            <For each={row}>{(cell) => <Cell cell={cell} />}</For>
          </div>
        )}
      </For>
    </div>
  </div>
);
