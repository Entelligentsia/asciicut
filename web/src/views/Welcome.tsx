import type { Component } from "solid-js";
import { createSignal, For, Show } from "solid-js";
import { isTauri } from "@tauri-apps/api/core";
import { mmss } from "../lib/format";
import { recentCasts, relativeTime, type RecentEntry } from "../lib/prefs";

interface WelcomeProps {
  /** Load the bundled sample recording (desktop) — the primary action. */
  onSample: () => void;
  /** Present the native `.cast` picker (desktop) / the loaded cast (browser). */
  onOpen: () => void;
  /** Re-open a Recent entry by path. */
  onRecent: (entry: RecentEntry) => void;
  /** The already-loaded recording's name, when the host launched with one. */
  launchName?: string;
  /** A failure from the last open attempt, surfaced in place. */
  error?: string | null;
  /** Whether an open is in flight (disables the actions). */
  busy?: boolean;
}

/**
 * The welcome view — what the app shows when no cast is loaded (brief §3A).
 *
 * It exists to fix a specific defect: the shipped first launch rendered the
 * whole editor against an empty model, producing FIVE simultaneous negative
 * messages ("No segment selected", "No segments yet", "Drop a `.cast`", "Add a
 * segment", and a red `no cast loaded`) and no guidance at all. The fix is
 * structural rather than cosmetic — `App` does not render the editor chrome
 * until a cast is loaded, so all five disappear at once and none of them needs
 * an individual empty state.
 *
 * Nothing here is styled as an error: an app that has not been given a file yet
 * is not in a failure state. Only {@link WelcomeProps.error} — a real open that
 * really failed — gets error styling.
 *
 * Degrades honestly off the desktop shell: `open_sample` / `open_cast_path`
 * are Tauri-only, and a browser tab has no local paths, so in that mode the
 * card offers the recording the server already launched with and omits the
 * Recent list entirely rather than showing dead controls.
 */
export const Welcome: Component<WelcomeProps> = (props) => {
  // Read once at mount: the list only changes as a result of leaving this view.
  const [recent] = createSignal<RecentEntry[]>(isTauri() ? recentCasts() : []);
  const desktop = isTauri();

  return (
    <div class="welcome" data-testid="welcome">
      <div class="wel-card">
        <div class="wel-mascot-wrap">
          <img
            class="wel-mascot animate-float"
            src="/assets/logo_master_transparent.png"
            alt="Nibbles the Beaver — the asciicut mascot"
          />
        </div>
        <div class="wel-brand">
          <span class="dot" />
          <b>asciicut</b>
          <span class="tag u">cutting&nbsp;room</span>
        </div>

        <p class="wel-lede">
          Cut the dead air out of terminal recordings —
          <br />
          and <span class="hi">see what you're removing</span>.
        </p>

        <div class="wel-acts">
          <Show
            when={desktop}
            fallback={
              <button
                type="button"
                class="wel-btn primary"
                data-testid="welcome-open"
                disabled={props.busy}
                onClick={props.onOpen}
              >
                <span class="t">
                  <span class="ic">▸</span> Open{" "}
                  {props.launchName ?? "the loaded recording"}
                </span>
                <span class="s">
                  The recording this server was started with.
                  <br />
                  Opening other files needs the desktop app.
                </span>
              </button>
            }
          >
            <button
              type="button"
              class="wel-btn primary"
              data-testid="welcome-sample"
              disabled={props.busy}
              onClick={props.onSample}
            >
              <span class="t">
                <span class="ic">▸</span> Try the sample
              </span>
              <span class="s">
                A real 17-minute agent sprint.
                <br />
                Best way to see what asciicut does.
              </span>
            </button>
            <button
              type="button"
              class="wel-btn"
              data-testid="welcome-open"
              disabled={props.busy}
              onClick={props.onOpen}
            >
              <span class="t">
                <span class="ic">◈</span> Open a recording…
              </span>
              <span class="s">
                Choose a <code>.cast</code> file.
                <br />
                <span class="mut">⌘/Ctrl+O</span>
              </span>
            </button>
          </Show>
        </div>

        <Show when={desktop && recent().length === 0}>
          <div class="wel-recent-empty" data-testid="welcome-recent-empty">
            <img
              class="wel-empty-art"
              src="/assets/ui_empty_state_transparent.png"
              alt=""
            />
            <span class="mut">No recent recordings yet — open one to get started.</span>
          </div>
        </Show>

        <Show when={desktop && recent().length > 0}>
          <div class="wel-recent" data-testid="welcome-recent">
            <div class="rl u">Recent</div>
            <For each={recent()}>
              {(entry) => (
                <button
                  type="button"
                  class="wel-row"
                  data-testid="welcome-recent-row"
                  disabled={props.busy}
                  onClick={() => props.onRecent(entry)}
                >
                  <span class="mut">◈</span>
                  <span class="nm">{entry.name}</span>
                  <span class="mt">
                    {entry.durationSecs !== undefined
                      ? `${mmss(entry.durationSecs)} · `
                      : ""}
                    {relativeTime(entry.openedAt)}
                  </span>
                </button>
              )}
            </For>
          </div>
        </Show>

        <Show when={props.error}>
          {(msg) => (
            <p class="error" data-testid="welcome-error" role="alert">
              {msg()}
            </p>
          )}
        </Show>

        <div class="wel-foot">
          asciicut 0.1.0 · MIT © Entelligentsia · press{" "}
          <b class="amber">?</b> in the editor for keyboard shortcuts
        </div>
      </div>
    </div>
  );
};
