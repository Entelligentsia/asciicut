import type { Component } from "solid-js";
import { createSignal, Show } from "solid-js";
import { isTauri } from "@tauri-apps/api/core";
import {
  cancelExportVideo,
  exportVideo,
  type ExportStage,
  type ExportVideoFormat,
} from "../lib/api";
import { onExportProgress } from "../lib/desktop";
import { editorStore } from "../lib/editorStore";

/** The three containers the desktop shell can render into. */
const FORMATS: readonly ExportVideoFormat[] = ["mp4", "webm", "gif"];

interface ExportDrawerProps {
  /** Whether the drawer is on-screen. */
  open: boolean;
  /** Dismiss the drawer (the ✕ button). */
  onClose: () => void;
  /** The source cast filename, for the derived output names. */
  srcName: string;
  /** The composed cut's duration label (`m:ss`), shown in the output summary. */
  cutLabel: string;
  /** The composed cut's duration in seconds — scales ffmpeg's progress bar. */
  cutDurationSecs: number;
  /** Whether a save/export-cast call is already in flight (browser path). */
  persistBusy: boolean;
  /** Write the composed `.cast` via `/api/export` (browser path only). */
  onExportCast: () => void;
  /** Save the project to its active path. */
  onSave: () => void;
  /** Surface a transient message to the user. */
  onToast: (msg: string) => void;
}

/**
 * The export drawer — the whole "turn this cut into a file" surface.
 *
 * Owns the desktop video pipeline's state (chosen format, busy/stage/percent,
 * terminal path or error) because nothing outside the drawer reads it: the
 * editor only opens and closes this sheet. The browser build has no bundled
 * `agg`/`ffmpeg` to drive, so it falls back to the `.cast` writer plus the
 * copyable command line — the `isTauri()` split, not a build-time flag, so one
 * bundle serves both.
 */
export const ExportDrawer: Component<ExportDrawerProps> = (props) => {
  const [format, setFormat] = createSignal<ExportVideoFormat>("mp4");
  const [busy, setBusy] = createSignal(false);
  const [stage, setStage] = createSignal<ExportStage | null>(null);
  const [percent, setPercent] = createSignal<number | null>(null);
  const [videoPath, setVideoPath] = createSignal<string | null>(null);
  const [errorMsg, setErrorMsg] = createSignal<string | null>(null);

  /** `<stem>.<ext>` for the source cast — the derived output names. */
  const named = (ext: string): string =>
    props.srcName.replace(/\.cast$/, ext);

  /**
   * Run the desktop `export_video` pipeline: native save dialog, round-trip
   * triple, then the bundled agg → ffmpeg render, with the progress bar driven
   * by the `export-progress` event stream. The listener is scoped to this one
   * call (not app-lifetime like `startDesktopBridge`'s), so it is always
   * cleaned up in `finally`.
   */
  const renderVideo = async (): Promise<void> => {
    if (busy()) return;
    setBusy(true);
    setStage(null);
    setPercent(null);
    setVideoPath(null);
    setErrorMsg(null);

    const unlisten = await onExportProgress((payload) => {
      setStage(payload.stage);
      setPercent(payload.percent ?? null);
      if (payload.stage === "error") {
        setErrorMsg(payload.message ?? "unknown error");
      }
      if (payload.stage === "done" && payload.path) {
        setVideoPath(payload.path);
      }
    });

    try {
      const res = await exportVideo(
        JSON.stringify(editorStore.getProject()),
        format(),
        props.cutDurationSecs,
      );
      setVideoPath(res.videoPath);
      props.onToast(`▸ wrote ${res.videoPath}`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg === "cancelled") {
        props.onToast("▸ export cancelled");
      } else {
        setErrorMsg(msg);
      }
    } finally {
      unlisten();
      setBusy(false);
    }
  };

  /** Whether the chosen format runs the ffmpeg transcode after agg (SPEC §9). */
  const needsFfmpeg = (): boolean => format() !== "gif";

  /**
   * The progress line's label. Both long stages now report a real percentage:
   * `agg`'s per-frame render bar (parsed from its stdout) and `ffmpeg`'s
   * `-progress` stream.
   */
  const stageLabel = (): string => {
    const pct = percent();
    switch (stage()) {
      case "writing-project":
        return "Writing project…";
      case "writing-cast":
        return "Composing cast…";
      case "agg":
        return pct !== null
          ? `Rendering frames (agg)… ${Math.round(pct)}%`
          : "Rendering frames (agg)…";
      case "ffmpeg":
        return pct !== null
          ? `Encoding video (ffmpeg)… ${Math.round(pct)}%`
          : "Encoding video (ffmpeg)…";
      case "done":
        return "▸ done";
      case "cancelled":
        return "▸ cancelled";
      case "error":
        return "▸ failed";
      default:
        return "";
    }
  };

  /**
   * One monotonic 0–100 bar across both render stages. Frame-rendering (agg) is
   * the long pole, so it owns the first 90% of the bar and the ffmpeg transcode
   * the final 10%; a `.gif` export never runs ffmpeg, so agg owns the whole
   * bar. This keeps the fill moving through the phase that used to sit dead at
   * 0%, instead of only animating during the quick ffmpeg tail.
   */
  const AGG_SHARE = 0.9;
  const progressPct = (): number => {
    if (stage() === "done") return 100;
    const pct = percent() ?? 0;
    switch (stage()) {
      case "agg":
        return needsFfmpeg() ? pct * AGG_SHARE : pct;
      case "ffmpeg":
        return needsFfmpeg() ? AGG_SHARE * 100 + pct * (1 - AGG_SHARE) : pct;
      default:
        return 0;
    }
  };

  const copyCommand = (): void => {
    const cmd = `asciicut compose ${named(".asciicut.json")} > ${named(".cut.cast")}`;
    void navigator.clipboard
      ?.writeText(cmd)
      .then(() => props.onToast("▸ command copied"))
      .catch(() => props.onToast("▸ copy failed — select the command instead"));
  };

  return (
    <div
      class="drawer"
      classList={{ open: props.open }}
      role="dialog"
      aria-modal="true"
      aria-label="Export cut"
      data-testid="editor-drawer"
    >
      <div class="sheet">
        <div class="sheet-h">
          <span class="amber">◆</span>
          <b class="u">Export cut</b>
          <span class="mut" style={{ "font-size": "11px" }}>
            {isTauri()
              ? "renders with the bundled agg → ffmpeg"
              : "no magic — here's the command"}
          </span>
          <button
            class="x"
            data-testid="editor-drawer-close"
            aria-label="Close"
            onClick={props.onClose}
          >
            ✕
          </button>
        </div>
        <div class="sheet-b">
          <div class="out-row">
            <div class="out">
              <div class="ol u">cast</div>
              <div class="ov">{named(".composed.cast")}</div>
              <div class="os">
                {props.cutLabel} · {editorStore.model().segments.length} segments
              </div>
            </div>
            <div class="out">
              <div class="ol u">video</div>
              <div class="ov">{named(`.${format()}`)}</div>
              <div class="os">{isTauri() ? "mp4 · webm · gif" : "+ .webm · .gif (M4)"}</div>
            </div>
            <div class="out">
              <div class="ol u">project</div>
              <div class="ov">{named(".asciicut.json")}</div>
              <div class="os">round-trips</div>
            </div>
          </div>

          <Show
            when={isTauri()}
            fallback={
              <div class="cmd" data-testid="editor-cmd">
                <button class="copy" data-testid="editor-copy-cmd" onClick={copyCommand}>
                  copy
                </button>
                <span class="c"># 1 · compose the edit into a new cast (non-destructive)</span>
                {"\n"}asciicut compose <span class="f">{named(".asciicut.json")}</span>
                {" > "}
                <span class="f">{named(".cut.cast")}</span>
                {"\n\n"}
                <span class="c"># 2 · render video with the recording's own theme (M4)</span>
                {"\n"}agg --theme dracula --font-family "JetBrains Mono" --font-size 14{" "}
                <span class="a">--fps-cap</span> 24 {named(".cut.cast")} {named(".gif")}
              </div>
            }
          >
            <div class="fmt-row" data-testid="editor-export-formats">
              {FORMATS.map((f) => (
                <button
                  class="fmt-btn"
                  classList={{ active: format() === f }}
                  data-testid={`editor-export-format-${f}`}
                  disabled={busy()}
                  onClick={() => setFormat(f)}
                >
                  .{f}
                </button>
              ))}
            </div>

            <Show when={busy() || stage() !== null}>
              <div class="progress-wrap" data-testid="editor-export-progress">
                <Show when={busy() && stage() !== "done" && stage() !== "error"}>
                  <img
                    class="export-mascot"
                    src="/assets/ui_splice_operator.gif"
                    alt=""
                  />
                </Show>
                <div class="progress-bar">
                  <div
                    class="progress-fill"
                    classList={{ error: stage() === "error", done: stage() === "done" }}
                    style={{ width: `${progressPct()}%` }}
                  />
                </div>
                <span class="progress-label" data-testid="editor-export-stage">
                  {stageLabel()}
                </span>
              </div>
            </Show>

            <Show when={stage() === "done" && videoPath()}>
              {(path) => (
                <p class="notice persist-line" data-testid="editor-export-result">
                  ▸ wrote {path()}
                </p>
              )}
            </Show>
            <Show when={errorMsg()}>
              {(msg) => (
                <p class="error persist-line" data-testid="editor-export-error" role="alert">
                  Export failed: {msg()}
                </p>
              )}
            </Show>
          </Show>
        </div>
        <div class="sheet-f">
          <Show
            when={isTauri()}
            fallback={
              <>
                <button
                  class="go"
                  data-testid="editor-do-export"
                  disabled={props.persistBusy}
                  onClick={() => {
                    props.onClose();
                    props.onExportCast();
                  }}
                >
                  ▸ Write cut &amp; render
                </button>
                <button
                  class="btn"
                  data-testid="editor-save"
                  disabled={props.persistBusy}
                  onClick={props.onSave}
                >
                  Save project
                </button>
                <span class="note">
                  composed cast writes now ·{" "}
                  <span class="amber">est. {props.cutLabel} clip</span>
                </span>
              </>
            }
          >
            <Show
              when={!busy()}
              fallback={
                <button
                  class="btn"
                  data-testid="editor-cancel-export"
                  onClick={() => void cancelExportVideo()}
                >
                  ✕ Cancel render
                </button>
              }
            >
              <button
                class="go"
                data-testid="editor-do-export"
                onClick={() => void renderVideo()}
              >
                ▸ Render {format()}
              </button>
            </Show>
            <button
              class="btn"
              data-testid="editor-save"
              disabled={props.persistBusy || busy()}
              onClick={props.onSave}
            >
              Save project
            </button>
            <span class="note">
              est. <span class="amber">{props.cutLabel}</span> clip · bundled agg → ffmpeg
            </span>
          </Show>
        </div>
      </div>
    </div>
  );
};
