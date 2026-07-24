// Framework-agnostic adapter over asciinema-player v3 (SPEC §7 web-preview row).
//
// This is the single seam between the SPA and the upstream player API. It owns
// the DATA-SOURCE-SHAPE decision and normalises the imperative controls the
// preview needs (seek / play / pause / dispose) into a small `PlayerHandle`, so
// callers (T10 `<PlayerPreview>`, and T12's composed-edit preview) never import
// `asciinema-player` directly and stay agnostic to how the recording is fed.
//
// Data source (AC#2): the cast text is fed IN-MEMORY through the player's
// `{ data }` `DataSource` form (`parser: "asciicast"`) — no network fetch, no
// object URL. v3's `index.d.ts` types the imperative `seek()` on the `Player`
// interface independent of the source shape, so the pure in-memory form
// supports scrubbing; validated in-browser, so NO Blob/object-URL fallback is
// required. Should a future asciicast dialect ever defeat the in-memory parse,
// this adapter is the one place to swap the source shape.
//
// Types: v3.17.0 ships first-class TypeScript declarations (`index.d.ts`), so
// there is NO local ambient `asciinema-player.d.ts` — the shipped `create` /
// `Player` / `DataSource` / `SeekLocation` types are imported directly. (The
// plan anticipated a hand-written ambient mirror; the package now makes that
// redundant — a local re-declaration would only risk drifting from upstream.)
import { create, type Options, type Player } from "asciinema-player";

/**
 * A minimal, framework-agnostic handle over one mounted player instance. All
 * imperative controls return a promise resolving when the player has applied
 * the command (v3's `seek`/`play`/`pause` are async).
 */
export interface PlayerHandle {
  /** Seek the playhead to an absolute time in seconds. */
  seek(time: number): Promise<void>;
  /** Begin playback. */
  play(): Promise<void>;
  /** Pause playback. */
  pause(): Promise<void>;
  /** The player's current playback time in seconds (cut timeline). */
  currentTime(): number;
  /** The composed recording's total duration in seconds, or `undefined` pre-parse. */
  duration(): number | undefined;
  /** Subscribe to a lifecycle event (`play`/`pause`/`ended`). */
  on(event: "play" | "pause" | "ended", handler: () => void): () => void;
  /** Tear down the player and release its DOM + resources. */
  dispose(): void;
}

/** The subset of player options the preview surface exposes. */
export interface MountOptions {
  /** Terminal columns override (defaults to the cast header). */
  cols?: number;
  /** Terminal rows override (defaults to the cast header). */
  rows?: number;
  /** Autoplay on mount. Default `false` — the preview is scrub-driven. */
  autoPlay?: boolean;
  /** How the terminal fits its container. Default `"width"`. */
  fit?: Options["fit"];
}

/**
 * Mount an asciinema-player v3 into `el` from an IN-MEMORY asciicast document.
 *
 * `create()` is synchronous but its internal `init()` parses the recording
 * asynchronously; a malformed cast surfaces there and is swallowed by the
 * player's own logger rather than thrown here. A synchronous construction
 * failure (e.g. an unusable source shape) DOES throw — callers must contain it
 * and surface an error state.
 *
 * @throws if the player cannot be constructed from `castText`.
 */
export function mountPlayer(
  el: HTMLElement,
  castText: string,
  opts: MountOptions = {},
): PlayerHandle {
  const player: Player = create(
    { data: castText, parser: "asciicast" },
    el,
    {
      autoPlay: opts.autoPlay ?? false,
      cols: opts.cols,
      rows: opts.rows,
      fit: opts.fit ?? "width",
      controls: true,
    },
  );

  const off = new Map<"play" | "pause" | "ended", (() => void)[]>();
  return {
    async seek(time: number): Promise<void> {
      await player.seek(time);
    },
    async play(): Promise<void> {
      await player.play();
    },
    async pause(): Promise<void> {
      await player.pause();
    },
    currentTime(): number {
      return player.getCurrentTime();
    },
    duration(): number | undefined {
      return player.getDuration();
    },
    on(event, handler) {
      player.addEventListener(event, handler);
      const list = off.get(event) ?? [];
      list.push(handler);
      off.set(event, list);
      return () => {
        const arr = off.get(event);
        if (!arr) return;
        off.set(event, arr.filter((h) => h !== handler));
      };
    },
    dispose(): void {
      player.dispose();
    },
  };
}
