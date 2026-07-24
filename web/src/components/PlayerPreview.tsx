import type { Component } from "solid-js";
import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { mountPlayer, type PlayerHandle } from "../lib/player";
import { setPlayerState } from "../lib/automation";

interface PlayerPreviewProps {
  /**
   * The IN-MEMORY asciicast document to preview. Empty / `null` / `undefined`
   * renders the empty state; a new non-empty value re-mounts the player.
   */
  data?: string | null;
  /** Optional externally-driven seek target in seconds (does not re-mount). */
  seek?: number;
  /** Imperative play/pause: `true` plays, `false` pauses. Re-mount resets to paused. */
  playing?: boolean;
  /** Fired each animation frame with the player's current cut-timeline time. */
  onTime?: (t: number) => void;
  /** Fired once the player reports its composed duration (after mount/parse). */
  onDuration?: (d: number | undefined) => void;
  /** Fired on player lifecycle transitions: `playing` | `paused` | `ended`. */
  onState?: (s: "playing" | "paused" | "ended") => void;
}

/**
 * In-memory recording preview (Objective, AC#1/#2/#4).
 *
 * Owns a container `<div ref>`, mounts an asciinema-player v3 through the
 * framework-agnostic `mountPlayer` adapter from the cast text passed in `data`,
 * and tears it down on unmount / data change. A `createEffect` re-mounts on
 * `data` change; a second drives `seek` imperatively WITHOUT a re-mount; a third
 * drives play/pause from the `playing` prop, running a RAF loop that reports the
 * current cut-timeline time via `onTime` (the transport clock). Loading / empty /
 * error states follow the SPA conventions (`data-testid` seams, `.notice` /
 * `.error role="alert"`) shared with `Editor.tsx` / `LoadCast.tsx`. The player is
 * opaque to the a11y snapshot, so mount/seek state is mirrored onto
 * `window.asciicut.player` for E2E.
 */
export const PlayerPreview: Component<PlayerPreviewProps> = (props) => {
  let container!: HTMLDivElement;
  let handle: PlayerHandle | null = null;
  let mountedBytes = 0;
  let raf = 0;
  const [error, setError] = createSignal<string | null>(null);
  const [mounted, setMounted] = createSignal(false);

  function teardown(): void {
    if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
    if (handle) {
      handle.dispose();
      handle = null;
    }
    mountedBytes = 0;
    setMounted(false);
    setPlayerState(null);
  }

  function remount(castText: string): void {
    teardown();
    setError(null);
    try {
      // `fit: "both"` scales the terminal to fit the bounded preview frame in
      // BOTH dimensions, so the whole recording stays visible inside the
      // single-screen layout instead of overflowing a fixed-width mount.
      handle = mountPlayer(container, castText, { fit: "both" });
      mountedBytes = castText.length;
      setMounted(true);
      setPlayerState({ mounted: true, bytes: mountedBytes, lastSeek: null });
      // The player parses async; report duration once known (poll a few frames).
      let polls = 0;
      const reportDur = () => {
        if (!handle) return;
        const d = handle.duration();
        if (d !== undefined || ++polls > 30) {
          props.onDuration?.(d);
        } else {
          requestAnimationFrame(reportDur);
        }
      };
      requestAnimationFrame(reportDur);
      // Lifecycle → onState + RAF time reporting while playing.
      const startClock = () => {
        if (raf) return;
        const tick = () => {
          if (!handle) return;
          props.onTime?.(handle.currentTime());
          raf = requestAnimationFrame(tick);
        };
        raf = requestAnimationFrame(tick);
      };
      const stopClock = () => {
        if (raf) {
          cancelAnimationFrame(raf);
          raf = 0;
        }
      };
      handle.on("play", () => {
        props.onState?.("playing");
        startClock();
      });
      handle.on("pause", () => {
        props.onState?.("paused");
        stopClock();
        if (handle) props.onTime?.(handle.currentTime());
      });
      handle.on("ended", () => {
        props.onState?.("ended");
        stopClock();
      });
    } catch (err) {
      handle = null;
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  // Mount / re-mount whenever the in-memory cast text changes.
  createEffect(() => {
    const text = props.data;
    if (text && text.length > 0) {
      remount(text);
    } else {
      teardown();
    }
  });

  // Drive seek imperatively on a mounted player, without re-mounting it.
  createEffect(() => {
    const target = props.seek;
    if (handle && typeof target === "number") {
      void handle
        .seek(target)
        .then(() =>
          setPlayerState({ mounted: true, bytes: mountedBytes, lastSeek: target }),
        )
        .catch((err: unknown) =>
          console.error("[asciicut] player seek failed", err),
        );
    }
  });

  // Drive play/pause from the `playing` prop (the transport Play button).
  createEffect(() => {
    const want = props.playing;
    if (!handle) return;
    if (want === undefined) return;
    if (want) {
      void handle.play().catch((err: unknown) =>
        console.error("[asciicut] player play failed", err),
      );
    } else {
      void handle.pause().catch((err: unknown) =>
        console.error("[asciicut] player pause failed", err),
      );
    }
  });

  onCleanup(teardown);

  return (
    <div class="player-preview" data-testid="player-preview">
      <Show when={error()}>
        {(msg) => (
          <p class="error" data-testid="player-error" role="alert">
            {msg()}
          </p>
        )}
      </Show>

      <Show when={!error() && !mounted()}>
        <p class="notice" data-testid="player-empty">
          Drop a <code>.cast</code> to preview the recording here.
        </p>
      </Show>

      {/* Always in the DOM so the ref is live before mount; hidden until used. */}
      <div
        ref={container}
        class="player-preview__mount"
        data-testid="player-mount"
        classList={{ "player-preview__mount--hidden": !mounted() }}
      />
    </div>
  );
};