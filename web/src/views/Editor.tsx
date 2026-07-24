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
import {
  composeProject,
  currentCastPath,
  exportProject,
  fetchActivity,
  fetchEventTimes,
  fetchFrame,
  fetchProject,
  openCast,
  openCastPath,
  openSample,
  quitNow,
  requestQuit,
  saveProject,
  saveProjectAs,
  confirmQuit,
  cancelQuit,
  type ActivitySignalDto,
  type FrameDto,
  type OpenedCast,
  type ProjectMeta,
  type SaveResponse,
} from "../lib/api";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { startDesktopBridge, type QuitTarget } from "../lib/desktop";
import { signalDuration, type Viewport } from "../lib/timeline";
import { editorStore } from "../lib/editorStore";
import { setSegmentsState, setTimelineState } from "../lib/automation";
import { cutToSource, scheduleOf, prevId, nextId } from "../lib/schedule";
import {
  remove,
  resizeEdge,
  segmentTag,
  setHoldEnd,
  setSpeed,
  moveByStep,
  type EditSegment,
} from "../lib/segments";
import { snapBy } from "../lib/events";
import { peakScore } from "../lib/timeline";
import { laneStatusAt, type LaneStatus } from "../lib/laneStatus";
import { mmss } from "../lib/format";
import { trimmedPct } from "../lib/format";
import {
  coachSeen,
  hintsEnabled,
  markCoachSeen,
  rememberCast,
  setHintsEnabled,
} from "../lib/prefs";
import { Timeline } from "../components/Timeline";
import { ContactSheet, type StripMode } from "../components/ContactSheet";
import { SegmentTrack } from "../components/SegmentTrack";
import { SegmentInspector } from "../components/SegmentInspector";
import { FrameLightbox } from "../components/FrameLightbox";
import { PlayerPreview } from "../components/PlayerPreview";
import { FrameGrid } from "../components/FrameGrid";
import { ExportDrawer } from "../components/ExportDrawer";
import { Welcome } from "./Welcome";

/** Trailing debounce for the compose→preview loop (ms) — coalesces drag bursts. */
const COMPOSE_DEBOUNCE_MS = 200;

/** The `±` window `✂ cut here` opens around the scrubbed moment (seconds). */
const CUT_HERE_HALF = 6;

/** A transient toast message. */
interface Toast {
  id: number;
  msg: string;
  color?: string;
}

/** A source moment being inspected on the terminal (brief §3H / §3D). */
interface SourceInspect {
  t: number;
  frame: FrameDto | null;
  status: LaneStatus;
  /** Set when the moment is a boundary being nudged — badges `in @` / `out @`. */
  mark?: "in" | "out";
}

/**
 * Editor view — the cutting room, refactored to the v2 prototype
 * (`prototype/ux/v2.html`) per `engineering/docs/v2-ux-refactor-brief.md`.
 *
 * The functional core is untouched: `lib/segments` reducers, `editorStore`,
 * `lib/api`, `lib/automation`, `lib/schedule`, and the canvas `Timeline` /
 * `SegmentTrack` all behave exactly as they did — compose stays byte-identical
 * (SPEC §7.1). What changes is the shell around them:
 *
 *   • a **welcome view** replaces the five-empty-state first launch (§3A): the
 *     editor chrome is not rendered at all until a cast is loaded;
 *   • the header is **three zones with the raw→cut ratio as hero** (§3B);
 *   • the inspector is the compact **`SegmentInspector`** with a live window
 *     diagram, event-snapping IN/OUT nudge, and solo/loop (§3C/§3D/§3E);
 *   • the filmstrip is the per-segment **`ContactSheet`** with source/cut-order
 *     axis modes and drag-reorder (§3F/§3G);
 *   • the activity lane keeps its canvas zoom/pan (drag = pan, §4.1) and gains a
 *     discoverable **click-to-inspect** scrub with a kept/dead/unkept guide and
 *     a true-source-frame preview (§3H);
 *   • teaching copy **decays** behind a `hints on|off` toggle and a `? keys`
 *     chip (§3B/§3D).
 *
 * All `data-testid` seams and the `window.asciicut` §8.4 command surface are
 * preserved.
 */
export const Editor: Component = () => {
  // ─── Load / welcome gate ───────────────────────────────────────────────────
  const [loaded, setLoaded] = createSignal(false);
  const [launchName, setLaunchName] = createSignal<string | undefined>(undefined);
  const [openError, setOpenError] = createSignal<string | null>(null);
  const [opening, setOpening] = createSignal(false);

  // ─── Loaded-editor state ───────────────────────────────────────────────────
  const [signal, setSignal] = createSignal<ActivitySignalDto | null>(null);
  const [eventTimes, setEventTimes] = createSignal<number[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [view, setView] = createSignal<Viewport>({ start: 0, end: 1 });
  const [playhead, setPlayhead] = createSignal(0);
  // Live compose→preview state.
  const [composed, setComposed] = createSignal<string>("");
  const [previewError, setPreviewError] = createSignal<string | null>(null);
  // Persistence surface.
  const [persistError, setPersistError] = createSignal<string | null>(null);
  const [persistBusy, setPersistBusy] = createSignal(false);
  // Transport / player clock.
  const [playing, setPlaying] = createSignal(false);
  const [cutTime, setCutTime] = createSignal(0);
  const [cutTotal, setCutTotal] = createSignal<number | undefined>(undefined);
  // Solo / loop a single segment (§3E).
  const [soloId, setSoloId] = createSignal<string | null>(null);
  const [looping, setLooping] = createSignal(true);
  // Source-inspection mode (§3H) — a scrubbed source moment on the terminal.
  const [srcInspect, setSrcInspect] = createSignal<SourceInspect | null>(null);
  // Activity-lane hover guide (§3H).
  const [hoverT, setHoverT] = createSignal<number | null>(null);
  // Contact-sheet axis mode (§3G) + auto-switch bookkeeping.
  const [stripMode, setStripModeSig] = createSignal<StripMode>("source");
  const [autoSwitched, setAutoSwitched] = createSignal(false);
  let userStripMode: StripMode = "source";
  // Teaching-copy decay (§3B) + first-run coach (§3A).
  const [hintsOn, setHintsOn] = createSignal(hintsEnabled());
  const [coachOpen, setCoachOpen] = createSignal(false);
  const [keysOpen, setKeysOpen] = createSignal(false);
  // Contact-sheet lightbox (§3F).
  const [lightbox, setLightbox] = createSignal<{ seg: EditSegment; frame: FrameDto | null } | null>(
    null,
  );
  // Export drawer + toasts + quit guard modal.
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  const [toasts, setToasts] = createSignal<Toast[]>([]);
  const [quitPrompt, setQuitPrompt] = createSignal<{ target: QuitTarget } | null>(null);
  // Imperative seek target for the player (solo enter / loop re-seek).
  const [seekTarget, setSeekTarget] = createSignal<number | undefined>(undefined);
  let seeking = false;
  // Set while a loop re-seek has been issued but the player's clock has not yet
  // confirmed it landed (see the webview-safety note on the solo loop below).
  let awaitingLoopSeek = false;

  /**
   * Seek the player to `t`, forcing the `PlayerPreview` seek effect to re-run
   * even when `t` equals the last target (the loop case re-seeks to the same
   * `cutStart`). Clearing to `undefined` first makes Solid see two distinct
   * changes, so the second one always fires.
   */
  const reseek = (t: number): void => {
    seeking = true;
    setSeekTarget(undefined);
    setSeekTarget(t);
    setCutTime(t);
    setTimeout(() => (seeking = false), 60);
  };

  let toastId = 0;
  function toast(msg: string, color?: string): void {
    const id = ++toastId;
    setToasts((t) => [...t, { id, msg, color }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2400);
  }

  const duration = (): number => {
    const sig = signal();
    // Prefer the last real event time; fall back to the ceil-quantized signal
    // span. The contact sheet fetches its own frames, so there are no thumbs to
    // derive the basis from any more — the event list is the honest last-event.
    if (!sig) return 0;
    const evs = eventTimes();
    const lastEvent = evs.length > 0 ? evs[evs.length - 1] : 0;
    return lastEvent > 0 ? lastEvent : Math.max(signalDuration(sig), 0.25);
  };

  const hasWaveform = (): boolean => {
    const sig = signal();
    return sig !== null && sig.buckets.length > 0;
  };

  // Approximate cut schedule + total (drives the clapper ratio + transport).
  const sched = () => scheduleOf(editorStore.model());
  const cutDur = (): number => cutTotal() ?? sched().total;

  const selected = (): EditSegment | undefined =>
    editorStore.model().segments.find((s) => s.id === editorStore.selectedId());

  // The soloed segment's schedule entry (cutStart / cutDur), when soloing.
  const soloSched = createMemo(() => {
    const id = soloId();
    if (id === null) return null;
    return sched().segments.find((s) => s.id === id) ?? null;
  });

  // ─── Load orchestration ────────────────────────────────────────────────────

  /** Hydrate the editor from a `ProjectMeta` plus its activity + event grid. */
  const loadFromMeta = (
    meta: ProjectMeta,
    sig: ActivitySignalDto,
    events: number[],
  ): void => {
    setSignal(sig);
    setEventTimes(events);
    const d = (() => {
      const lastEvent = events.length > 0 ? events[events.length - 1] : 0;
      return lastEvent > 0 ? lastEvent : Math.max(signalDuration(sig), 0.25);
    })();
    setView({ start: 0, end: d > 0 ? d : 1 });
    editorStore.reset();
    if (meta.project) {
      editorStore.loadProject(meta.project);
    } else if (d > 0) {
      editorStore.setDuration(d);
      editorStore.addSegment({ srcStart: 0, srcEnd: d, label: "full recording" });
      editorStore.markClean();
    }
    editorStore.setSource(meta.source);
    setLaunchName(meta.source);
    setLoaded(true);
    // First-run coach mark, once, anchored on the segment lane (§3A).
    if (!coachSeen()) setCoachOpen(true);
  };

  /** Fetch the activity + event grid for the currently loaded session cast. */
  const fetchSignals = async (): Promise<[ActivitySignalDto, number[]]> =>
    Promise.all([fetchActivity(), fetchEventTimes()]);

  /** Apply a desktop `OpenedCast` (dialog / recent / sample) and record it. */
  const applyOpened = async (opened: OpenedCast): Promise<void> => {
    const [sig, events] = await fetchSignals();
    loadFromMeta(opened.meta, sig, events);
    rememberCast(opened.path, opened.meta.source, duration());
  };

  /** Welcome → "Open a recording…" (native dialog on desktop; server cast in browser). */
  const doOpen = async (): Promise<void> => {
    if (opening()) return;
    setOpening(true);
    setOpenError(null);
    try {
      if (isTauri()) {
        const opened = await openCast();
        if (!opened) return; // cancelled
        await applyOpened(opened);
        toast("▸ cast opened");
      } else {
        // Browser demo: the server already launched with a cast — load it.
        const meta = await fetchProject();
        const [sig, events] = await fetchSignals();
        loadFromMeta(meta, sig, events);
        toast("▸ cast loaded");
      }
    } catch (err) {
      setOpenError(err instanceof Error ? err.message : String(err));
    } finally {
      setOpening(false);
    }
  };

  /** Welcome → "Try the sample" (desktop-only bundled recording). */
  const doSample = async (): Promise<void> => {
    if (opening()) return;
    setOpening(true);
    setOpenError(null);
    try {
      const opened = await openSample();
      await applyOpened(opened);
      toast("▸ sample loaded");
    } catch (err) {
      setOpenError(err instanceof Error ? err.message : String(err));
    } finally {
      setOpening(false);
    }
  };

  /** Welcome → a Recent entry (desktop-only). */
  const doRecent = async (path: string): Promise<void> => {
    if (opening()) return;
    setOpening(true);
    setOpenError(null);
    try {
      const opened = await openCastPath(path);
      await applyOpened(opened);
      toast("▸ cast opened");
    } catch (err) {
      // A moved/deleted file surfaces here (not "cancelled"); tell the user.
      setOpenError(err instanceof Error ? err.message : String(err));
    } finally {
      setOpening(false);
    }
  };

  // ─── Persistence ───────────────────────────────────────────────────────────

  const updateTitle = async (): Promise<void> => {
    const src = editorStore.source();
    if (!src) return;
    const title = editorStore.isDirty() ? `● ${src}` : src;
    try {
      await getCurrentWindow().setTitle(`asciicut — ${title}`);
    } catch {
      // Not in Tauri — ignore.
    }
  };

  const doSave = async (): Promise<SaveResponse | null> => {
    if (persistBusy()) return null;
    setPersistBusy(true);
    setPersistError(null);
    try {
      const res = await saveProject(JSON.stringify(editorStore.getProject()));
      editorStore.markClean();
      toast("▸ project saved");
      return res;
    } catch (err) {
      setPersistError(err instanceof Error ? err.message : String(err));
      return null;
    } finally {
      setPersistBusy(false);
    }
  };

  const doSaveAs = async (): Promise<SaveResponse | null> => {
    if (persistBusy()) return null;
    setPersistBusy(true);
    setPersistError(null);
    try {
      const res = await saveProjectAs(JSON.stringify(editorStore.getProject()));
      if (res) {
        editorStore.markClean();
        toast("▸ project saved as");
      }
      return res;
    } catch (err) {
      setPersistError(err instanceof Error ? err.message : String(err));
      return null;
    } finally {
      setPersistBusy(false);
    }
  };

  const doQuit = (target: QuitTarget): void => {
    if (!editorStore.isDirty()) {
      void quitNow();
      return;
    }
    setQuitPrompt({ target });
  };

  const closeQuitPrompt = (): void => {
    setQuitPrompt(null);
    void cancelQuit();
  };
  const onQuitSave = async (): Promise<void> => {
    setQuitPrompt(null);
    const res = await doSave();
    if (res !== null) await confirmQuit();
    else await cancelQuit();
  };
  const onQuitDiscard = async (): Promise<void> => {
    setQuitPrompt(null);
    await confirmQuit();
  };

  // ─── Startup ───────────────────────────────────────────────────────────────
  onMount(() => {
    let cleanupBridge: (() => void) | undefined;

    void (async () => {
      try {
        cleanupBridge = await startDesktopBridge({
          openCast: doOpen,
          save: doSave,
          saveAs: doSaveAs,
          export: openExportDrawer,
          requestQuit,
          confirmQuit: doQuit,
          onCastOpened: (opened) => {
            void (async () => {
              const [sig, events] = await fetchSignals();
              loadFromMeta(opened.meta, sig, events);
              rememberCast(opened.path, opened.meta.source, duration());
            })();
          },
        });

        if (isTauri()) {
          // Desktop: a launch-argument cast is already loaded — skip straight to
          // the editor and record it. No argument → the welcome view stands.
          const path = await currentCastPath();
          if (path) {
            const [meta, [sig, events]] = await Promise.all([
              fetchProject(),
              fetchSignals(),
            ]);
            loadFromMeta(meta, sig, events);
            rememberCast(path, meta.source, duration());
          } else {
            // Populate the welcome button's name if the shell knows one.
            try {
              setLaunchName((await fetchProject()).source);
            } catch {
              // Empty session — the welcome view offers the sample instead.
            }
          }
        } else {
          // Browser demo: the server always has a cast and has no welcome-only
          // affordances (no sample command, no recent paths), so open straight
          // into the editor — matching the historical browser behaviour and
          // keeping the `window.asciicut.open()` automation contract (which
          // resolves on `segments != null`) working without a manual click.
          const [meta, [sig, events]] = await Promise.all([
            fetchProject(),
            fetchSignals(),
          ]);
          loadFromMeta(meta, sig, events);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    })();

    onCleanup(() => cleanupBridge?.());
  });

  // Keep the store's duration in sync (only meaningful once loaded).
  createEffect(() => {
    if (loaded()) editorStore.setDuration(duration());
  });

  // Reflect source + dirty state in the native window title (Tauri only).
  createEffect(() => {
    void editorStore.source();
    void editorStore.isDirty();
    void updateTitle();
  });

  createEffect(() => {
    const sig = signal();
    setTimelineState(
      sig
        ? { playhead: playhead(), view: [view().start, view().end], bucketCount: sig.buckets.length }
        : null,
    );
  });

  createEffect(() => {
    const m = editorStore.model();
    setSegmentsState(
      signal()
        ? {
            count: m.segments.length,
            selectedId: editorStore.selectedId(),
            idleCap: m.idleCap,
            segments: m.segments.map((s) => ({
              id: s.id,
              srcStart: s.srcStart,
              srcEnd: s.srcEnd,
              speed: s.speed,
              holdEnd: s.holdEnd,
            })),
          }
        : null,
    );
  });

  // Live preview compose loop: debounced, generation-guarded.
  let composeGen = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    if (!loaded()) return;
    const project = editorStore.getProject();
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    if (project.segments.length === 0) {
      setComposed("");
      setPreviewError(null);
      return;
    }
    const projectJson = JSON.stringify({ ...project, source: project.source || "launch.cast" });
    const gen = ++composeGen;
    debounceTimer = setTimeout(() => {
      void (async () => {
        try {
          const text = await composeProject(projectJson);
          if (gen !== composeGen) return;
          setComposed(text);
          setPreviewError(null);
        } catch (err) {
          if (gen !== composeGen) return;
          setPreviewError(err instanceof Error ? err.message : String(err));
        }
      })();
    }, COMPOSE_DEBOUNCE_MS);
  });
  onCleanup(() => {
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
  });

  // ─── Transport ─────────────────────────────────────────────────────────────

  // Player time → playhead (source) + transport line + solo bounds.
  const onTime = (t: number): void => {
    setCutTime(t);
    if (seeking) return;
    // Solo: stop (or loop) once the segment's cut range is exhausted (§3E).
    const solo = soloSched();
    if (solo) {
      const end = solo.cutStart + solo.cutDur;
      // ─── WEBVIEW-SAFE LOOP (fixes a WKWebView/WebKitGTK hang) ──────────────
      // The player keeps playing the full composed cut; we yank it back to
      // `cutStart` at the segment's end. On slower webviews `seek()` is async
      // and `currentTime()` reports the pre-seek (past-end) time for several
      // frames afterwards. Re-issuing `seek()` every one of those frames
      // cancels the in-flight seek so it never lands — a livelock that pins the
      // compositor and reads as a frozen UI (Chromium's seek is fast enough to
      // land inside the guard window, so it never surfaces there). So issue at
      // most ONE loop-seek per cycle (`awaitingLoopSeek`) and only re-arm once
      // the clock confirms we are back inside the segment.
      if (awaitingLoopSeek) {
        if (t < end - 0.1) awaitingLoopSeek = false;
        return;
      }
      if (t >= end - 0.02) {
        if (looping()) {
          awaitingLoopSeek = true;
          reseek(solo.cutStart);
          return;
        }
        exitSolo();
        setPlaying(false);
        return;
      }
    }
    const mapped = cutToSource(sched(), t);
    if (mapped) setPlayhead(mapped.srcTime);
  };
  const onDuration = (d: number | undefined): void => {
    setCutTotal(d);
  };
  const onState = (s: "playing" | "paused" | "ended"): void => {
    setPlaying(s === "playing");
    if (s === "ended") {
      const mapped = cutToSource(sched(), cutDur());
      if (mapped) setPlayhead(mapped.srcTime);
      if (soloId() !== null) exitSolo();
      toast("▸ playback ended");
    }
  };

  const clearSourceInspect = (): void => {
    setSrcInspect(null);
  };

  const togglePlay = (): void => {
    // Play leaves source-inspection (§3H) and exits any solo scope.
    clearSourceInspect();
    if (soloId() !== null) exitSolo();
    const next = !playing();
    setPlaying(next);
    if (next) {
      // Playback switches the strip to cut order for coherence (§3G).
      autoStripForPlayback(true);
      toast("▸ playing composed cut — watch it skip the wait");
    } else {
      autoStripForPlayback(false);
    }
  };

  // Timeline click → SOURCE INSPECTION (§3H): show the true source frame at T,
  // not a composed frame. This is a distinct mode from cut playback.
  const onScrub = (srcTime: number): void => {
    setPlaying(false);
    if (soloId() !== null) exitSolo();
    setPlayhead(srcTime);
    const status = laneStatusAt(signal()!, srcTime, keptTagAt, peakScore(signal()!));
    setSrcInspect({ t: srcTime, frame: null, status });
    void fetchFrame(srcTime)
      .then((frame) =>
        setSrcInspect((cur) => (cur && cur.t === srcTime ? { ...cur, frame } : cur)),
      )
      .catch(() => {});
  };

  // ─── Solo (§3E) ────────────────────────────────────────────────────────────
  const enterSolo = (id: string): void => {
    clearSourceInspect();
    editorStore.setSelectedId(id);
    setSoloId(id);
    const s = sched().segments.find((x) => x.id === id);
    if (!s) return;
    awaitingLoopSeek = false;
    reseek(s.cutStart);
    setPlaying(true);
    toast(`▸ soloing ${tagOf(id)}${looping() ? " · looping" : ""}`);
  };
  const exitSolo = (): void => {
    awaitingLoopSeek = false;
    setSoloId(null);
  };
  const toggleSolo = (id: string): void => {
    if (soloId() === id) {
      exitSolo();
      setPlaying(false);
    } else {
      enterSolo(id);
    }
  };

  const tagOf = (id: string | null): string => {
    const s = editorStore.model().segments.find((x) => x.id === id);
    return s ? segmentTag(s) : "segment";
  };

  // ─── Strip auto-switch (§3G) ───────────────────────────────────────────────
  const setStripMode = (m: StripMode, auto = false): void => {
    setStripModeSig(m);
    if (!auto) {
      userStripMode = m;
      setAutoSwitched(false);
    }
  };
  const onUserStripMode = (m: StripMode): void => {
    // A manual toggle during playback wins and disables auto-management.
    setStripMode(m);
  };
  const autoStripForPlayback = (on: boolean): void => {
    if (on) {
      if (stripMode() !== "cut") {
        setAutoSwitched(true);
        setStripMode("cut", true);
      }
    } else if (autoSwitched()) {
      setAutoSwitched(false);
      setStripMode(userStripMode, true);
    }
  };

  // ─── Contact sheet actions ─────────────────────────────────────────────────
  const onEnlarge = (seg: EditSegment, frame: FrameDto | null): void => {
    setLightbox({ seg, frame });
    // The card's cache may still be loading; fetch the true median frame if so.
    if (!frame) {
      const t = (seg.srcStart + seg.srcEnd) / 2;
      void fetchFrame(t)
        .then((f) =>
          setLightbox((cur) => (cur && cur.seg.id === seg.id ? { ...cur, frame: f } : cur)),
        )
        .catch(() => {});
    }
  };

  // ─── Source-frame status helpers ───────────────────────────────────────────
  /** The display tag of the segment covering source time `t`, or null. */
  const keptTagAt = (t: number): string | null => {
    const segs = editorStore.model().segments;
    for (let i = segs.length - 1; i >= 0; i--) {
      if (t >= segs[i].srcStart && t <= segs[i].srcEnd) return segmentTag(segs[i]);
    }
    return null;
  };

  const hoverStatus = createMemo<{ t: number; status: LaneStatus } | null>(() => {
    const t = hoverT();
    const sig = signal();
    if (t === null || !sig) return null;
    return { t, status: laneStatusAt(sig, t, keptTagAt, peakScore(sig)) };
  });

  // ─── `✂ cut here` (§3H) ────────────────────────────────────────────────────
  const cutHere = (): void => {
    const insp = srcInspect();
    if (!insp) return;
    const a = Math.max(0, insp.t - CUT_HERE_HALF);
    const b = Math.min(duration(), insp.t + CUT_HERE_HALF);
    const label = insp.status.kind === "unkept" ? "unkept activity" : "new window";
    const id = editorStore.addSegment({ srcStart: a, srcEnd: b, label });
    clearSourceInspect();
    toast(`✂ cut added at ${mmss(insp.t)}`);
    void id;
  };

  // ─── Export / drawer ───────────────────────────────────────────────────────
  const doExport = (): void => {
    if (persistBusy()) return;
    setPersistBusy(true);
    setPersistError(null);
    void (async () => {
      try {
        const res = await exportProject(JSON.stringify(editorStore.getProject()));
        toast(`▸ wrote ${res.castPath} · ${mmss(cutDur())} cut`);
      } catch (err) {
        setPersistError(err instanceof Error ? err.message : String(err));
      } finally {
        setPersistBusy(false);
      }
    })();
  };
  const openExportDrawer = (): void => {
    setDrawerOpen(true);
  };
  const closeExportDrawer = (): void => {
    setDrawerOpen(false);
  };

  // ─── Hints toggle (§3B) ────────────────────────────────────────────────────
  const toggleHints = (): void => {
    const next = !hintsOn();
    setHintsOn(next);
    setHintsEnabled(next);
  };

  const dismissCoach = (): void => {
    setCoachOpen(false);
    markCoachSeen();
  };

  // ─── Keyboard shortcuts ────────────────────────────────────────────────────
  const onKeyDown = (e: KeyboardEvent): void => {
    if (!loaded()) return;
    const tgt = e.target as HTMLElement | null;
    if (tgt && (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA")) return;
    const m = editorStore.model();
    const s = m.segments.find((x) => x.id === editorStore.selectedId());
    const sc = scheduleOf(m);
    switch (e.key) {
      case " ":
        e.preventDefault();
        togglePlay();
        break;
      case "Enter":
        if (editorStore.selectedId()) toggleSolo(editorStore.selectedId()!);
        break;
      case "]":
      case "}":
        if (e.shiftKey && s) {
          editorStore.setModel(moveByStep(m, s.id, 1));
          toast(`▸ ${tagOf(s.id)} moved later`);
        } else {
          editorStore.setSelectedId(nextId(sc, editorStore.selectedId()));
        }
        break;
      case "[":
      case "{":
        if (e.shiftKey && s) {
          editorStore.setModel(moveByStep(m, s.id, -1));
          toast(`▸ ${tagOf(s.id)} moved earlier`);
        } else {
          editorStore.setSelectedId(prevId(sc, editorStore.selectedId()));
        }
        break;
      case ",":
        nudgeSelected("in", e.shiftKey ? -10 : -1);
        break;
      case ".":
        nudgeSelected("in", e.shiftKey ? 10 : 1);
        break;
      case "s":
        if (s) {
          editorStore.setModel(setSpeed(m, s.id, Math.min(4, +(s.speed + 0.5).toFixed(1))));
          toast(`▸ ${tagOf(s.id)} speed → ${Math.min(4, s.speed + 0.5)}×`);
        }
        break;
      case "h":
        if (s) {
          editorStore.setModel(setHoldEnd(m, s.id, Math.min(5, +(s.holdEnd + 0.5).toFixed(1))));
          toast(`▸ ${tagOf(s.id)} hold → ${Math.min(5, s.holdEnd + 0.5)}s`);
        }
        break;
      case "x":
        if (s) {
          editorStore.setModel(remove(m, s.id));
          editorStore.setSelectedId(null);
          if (soloId() === s.id) exitSolo();
          toast(`▸ dropped ${tagOf(s.id)}`, "var(--red)");
        }
        break;
      case "e":
        openExportDrawer();
        break;
      case "?":
        setKeysOpen((v) => !v);
        break;
      case "Escape":
        setDrawerOpen(false);
        setLightbox(null);
        clearSourceInspect();
        break;
    }
  };

  /** Keyboard nudge of the selected segment's IN mark (`,` / `.`), snapped. */
  const nudgeSelected = (which: "in" | "out", steps: number): void => {
    const s = selected();
    if (!s) return;
    const from = which === "in" ? s.srcStart : s.srcEnd;
    const target = snapBy(eventTimes(), from, steps);
    if (target === from) return;
    const edge = which === "in" ? "left" : "right";
    const next = resizeEdge(editorStore.model(), s.id, edge, target, duration());
    editorStore.setModel(next);
    const landed = next.segments.find((x) => x.id === s.id);
    if (landed) onNudged(which, which === "in" ? landed.srcStart : landed.srcEnd);
  };

  /** A boundary was nudged — preview that exact source frame (§3D). */
  const onNudged = (which: "in" | "out", t: number): void => {
    setPlaying(false);
    if (soloId() !== null) exitSolo();
    setPlayhead(t);
    const status = laneStatusAt(signal()!, t, keptTagAt, peakScore(signal()!));
    setSrcInspect({ t, frame: null, status, mark: which });
    void fetchFrame(t)
      .then((frame) =>
        setSrcInspect((cur) => (cur && cur.t === t ? { ...cur, frame } : cur)),
      )
      .catch(() => {});
  };

  onMount(() => window.addEventListener("keydown", onKeyDown));
  onCleanup(() => window.removeEventListener("keydown", onKeyDown));

  const keepWindows = (): (readonly [number, number])[] =>
    editorStore.model().segments.map((s) => [s.srcStart, s.srcEnd] as const);

  const srcName = (): string => editorStore.source() || "launch.cast";
  const rawTime = (): number => duration();

  // Solo-scoped transport display (§3E): 0:04 / 0:32 within the segment.
  const transportNow = (): number => {
    const solo = soloSched();
    return solo ? Math.max(0, cutTime() - solo.cutStart) : cutTime();
  };
  const transportTotal = (): number => {
    const solo = soloSched();
    return solo ? solo.cutDur : cutDur();
  };

  // ─── Render ────────────────────────────────────────────────────────────────
  return (
    <Show
      when={loaded()}
      fallback={
        <Welcome
          onSample={() => void doSample()}
          onOpen={() => void doOpen()}
          onRecent={(entry) => void doRecent(entry.path)}
          launchName={launchName()}
          error={openError()}
          busy={opening()}
        />
      }
    >
      <div class="app" data-testid="editor">
        {/* HEADER — three zones, ratio as hero (§3B) */}
        <header class="clapper">
          <div class="brand">
            <img
              class="brand-logo"
              src="/assets/logomark_head_transparent.png"
              alt=""
              width="26"
              height="26"
            />
            <b>asciicut</b>
          </div>
          <div class="file">
            <Show when={editorStore.isDirty()}>
              <span class="dirty" title="Unsaved changes">
                ●
              </span>
            </Show>
            <span class="mut">◈</span>
            <span class="name">{srcName()}</span>
          </div>
          <div class="ratio">
            <span class="raw" data-testid="editor-raw-time">
              {mmss(rawTime())}
            </span>
            <span class="arrow">──▶</span>
            <span class="cut" data-testid="editor-cut-time">
              {mmss(cutDur())}
            </span>
            <span class="pct" data-testid="editor-pct-cut">
              {trimmedPct(rawTime(), cutDur())}
            </span>
          </div>
        </header>

        {/* BODY */}
        <main>
          {/* PREVIEW */}
          <section class="pane preview">
            <div class="pane-h">
              <span class="k u amber">◆ preview</span>
              <span class="rule" />
            </div>
            <div class="frame">
              <div class="titlebar">
                <span class="tl-dot r" />
                <span class="tl-dot y" />
                <span class="tl-dot g" />
                <span class="t">{srcName()} — composed</span>
                <Show when={soloId() !== null || srcInspect()}>
                  <span class="badge on" data-testid="preview-badge">
                    {(() => {
                      const insp = srcInspect();
                      if (insp) {
                        return insp.mark
                          ? `${insp.mark} @ ${mmss(insp.t)}`
                          : `source ${mmss(insp.t)}`;
                      }
                      return `solo ${tagOf(soloId())}`;
                    })()}
                  </span>
                </Show>
              </div>
              <div class="screen">
                <PlayerPreview
                  data={composed()}
                  seek={seekTarget()}
                  playing={playing()}
                  onTime={onTime}
                  onDuration={onDuration}
                  onState={onState}
                />
                {/* Source-inspection overlay: the TRUE frame at T (§3H). */}
                <Show when={srcInspect()}>
                  {(insp) => (
                    <div class="source-overlay" data-testid="source-overlay">
                      <Show
                        when={insp().frame}
                        fallback={<p class="notice">Reading the source frame…</p>}
                      >
                        {(frame) => <FrameGrid frame={frame()} />}
                      </Show>
                    </div>
                  )}
                </Show>
              </div>
            </div>
            <div class="transport">
              <button
                class="play"
                data-testid="editor-play"
                aria-label="Play or pause the composed cut"
                onClick={togglePlay}
              >
                {playing() ? "❚❚  Pause" : "▶  Play cut"}
              </button>
              <span class="tcode">
                <span data-testid="editor-playhead-time">{mmss(transportNow())}</span>
                <span class="sep">/</span>
                <span>{mmss(transportTotal())}</span>
              </span>
              <span class="seg-now" data-testid="editor-seg-now">
                {(() => {
                  const insp = srcInspect();
                  if (insp) {
                    return insp.status.kind === "kept"
                      ? `▸ ${insp.status.tag}`
                      : `▸ ${insp.status.tag} — not in any cut`;
                  }
                  const sc = sched();
                  const np = cutToSource(sc, cutTime());
                  if (!np) return "▸ no segments yet";
                  return `▸ ${segmentTag(np.seg)} · ${np.seg.label ?? "window"}`;
                })()}
              </span>
              <Show when={srcInspect() && srcInspect()!.status.kind !== "kept"}>
                <button
                  class="cuthere on"
                  data-testid="editor-cut-here"
                  onClick={cutHere}
                >
                  ✂ cut here
                </button>
              </Show>
              <span class="speed-now" data-testid="editor-speed-now">
                {(() => {
                  if (srcInspect()) return "src";
                  const np = cutToSource(sched(), cutTime());
                  return np ? `${np.seg.speed.toFixed(1)}×` : "—";
                })()}
              </span>
            </div>
            <Show when={previewError()}>
              {(msg) => (
                <p class="error" data-testid="editor-preview-error" role="alert">
                  Compose failed: {msg()}
                </p>
              )}
            </Show>
            <Show when={!composed() && !previewError()}>
              <p class="notice" data-testid="editor-preview-empty">
                Add a segment on the cuts lane to preview the composed recording.
              </p>
            </Show>
          </section>

          {/* INSPECTOR */}
          <section class="pane insp-pane">
            <SegmentInspector
              model={editorStore.model()}
              selectedId={editorStore.selectedId()}
              onModel={editorStore.setModel}
              duration={duration()}
              eventTimes={eventTimes()}
              soloing={soloId() !== null && soloId() === editorStore.selectedId()}
              onToggleSolo={() => {
                const id = editorStore.selectedId();
                if (id) toggleSolo(id);
              }}
              looping={looping()}
              onToggleLoop={() => setLooping((v) => !v)}
              hintsOn={hintsOn()}
              onNudged={onNudged}
            />
          </section>

          {/* TIMELINE + CONTACT SHEET */}
          <section class="timeline-hero">
            <div class="tl-head">
              <span class="k u amber">◆ activity</span>
              <Show when={hintsOn()}>
                <span class="k mut">
                  dead air is flat ·{" "}
                  <span class="cyan">click anywhere to inspect the source</span>
                </span>
              </Show>
              <div class="idle">
                <span class="u">idle&nbsp;cap</span>
                <input
                  type="range"
                  min="0.2"
                  max="2"
                  step="0.1"
                  value={editorStore.model().idleCap}
                  data-testid="segment-idlecap"
                  aria-label="Global idle cap in seconds"
                  onInput={(e) => {
                    const v = Number(e.currentTarget.value);
                    if (Number.isFinite(v)) editorStore.setIdleCap(v);
                  }}
                />
                <span class="idle-val" data-testid="segment-idlecap-value">
                  {editorStore.model().idleCap.toFixed(1)}s
                </span>
              </div>
            </div>

            <Show when={error()}>
              {(msg) => (
                <p class="error" data-testid="editor-error" role="alert">
                  {msg()}
                </p>
              )}
            </Show>
            <Show when={!error() && !hasWaveform()}>
              <p class="notice" data-testid="editor-empty">
                This recording has no activity to plot (an event-less cast).
              </p>
            </Show>

            <Show when={hasWaveform()}>
              <div class="ticks" data-testid="editor-ticks">
                <span>0:00</span>
                <span>{mmss(rawTime() / 4)}</span>
                <span>{mmss(rawTime() / 2)}</span>
                <span>{mmss((rawTime() / 4) * 3)}</span>
                <span>{mmss(rawTime())}</span>
              </div>
              <div class="wave-anchor">
              <div class="wave-wrap" data-testid="editor-wave-wrap">
                <Timeline
                  signal={signal()!}
                  view={view()}
                  setView={setView}
                  playhead={playhead()}
                  setPlayhead={onScrub}
                  duration={duration()}
                  keepWindows={keepWindows()}
                  onHover={setHoverT}
                />
                <SegmentTrack
                  model={editorStore.model()}
                  view={view()}
                  duration={duration()}
                  selectedId={editorStore.selectedId()}
                  onSelect={editorStore.setSelectedId}
                  onModel={editorStore.setModel}
                />
                {/* Solo mask: dim the timeline outside the soloed segment (§3E). */}
                <Show when={soloSched()}>
                  {(solo) => {
                    const span = (): number => view().end - view().start;
                    const pctL = (): number =>
                      span() > 0
                        ? ((editorStore.model().segments.find((s) => s.id === solo().id)!.srcStart -
                            view().start) /
                            span()) *
                          100
                        : 0;
                    const pctR = (): number =>
                      span() > 0
                        ? ((editorStore.model().segments.find((s) => s.id === solo().id)!.srcEnd -
                            view().start) /
                            span()) *
                          100
                        : 100;
                    return (
                      <>
                        <div
                          class="solomask"
                          style={{ left: "0", width: `${Math.max(0, pctL())}%` }}
                        />
                        <div
                          class="solomask"
                          style={{ left: `${Math.min(100, pctR())}%`, right: "0" }}
                        />
                      </>
                    );
                  }}
                </Show>
                {/* Hover guide + kept/dead/unkept chip (§3H). */}
                <Show when={hoverStatus()}>
                  {(hs) => {
                    const span = (): number => view().end - view().start;
                    const pct = (): number =>
                      span() > 0 ? ((hs().t - view().start) / span()) * 100 : 0;
                    return (
                      <Show when={pct() >= 0 && pct() <= 100}>
                        <div class="scrub on" style={{ left: `${pct()}%` }} />
                        <div
                          class="scrubchip on"
                          data-testid="editor-scrub-chip"
                          style={{ left: `${pct()}%` }}
                        >
                          {mmss(hs().t)}{" "}
                          <span
                            classList={{
                              kept: hs().status.kind === "kept",
                              dead: hs().status.kind === "dead",
                              miss: hs().status.kind === "unkept",
                            }}
                          >
                            · {hs().status.tag}
                          </span>
                        </div>
                      </Show>
                    );
                  }}
                </Show>
              </div>
                {/* First-run coach mark on the segment lane (§3A). Lives outside
                    .wave-wrap so its dismiss button is never clipped by that
                    lane's overflow:hidden; the .wave-anchor shares the lane's
                    top origin so the top:88px anchor is unchanged. */}
                <Show when={coachOpen()}>
                  <div class="coach" data-testid="editor-coach" style={{ left: "16px", top: "88px" }}>
                    This is your whole recording as one clip.
                    <br />
                    Drag its <b>edges to trim</b>, or drag on this lane to{" "}
                    <b>cut a new piece</b>.
                    <button class="dismiss" data-testid="editor-coach-dismiss" onClick={dismissCoach}>
                      Got it
                    </button>
                  </div>
                </Show>
              </div>

              <ContactSheet
                model={editorStore.model()}
                view={view()}
                duration={duration()}
                selectedId={editorStore.selectedId()}
                onSelect={editorStore.setSelectedId}
                onModel={editorStore.setModel}
                onSolo={toggleSolo}
                onEnlarge={onEnlarge}
                mode={stripMode()}
                onMode={onUserStripMode}
                autoSwitched={autoSwitched()}
                playingId={playing() ? cutToSource(sched(), cutTime())?.seg.id ?? null : null}
                hintsOn={hintsOn()}
              />
            </Show>
          </section>
        </main>

        {/* FOOTER — keybinds behind a chip, hints toggle (§3B) */}
        <footer class="status">
          <button
            class="chip"
            classList={{ on: keysOpen() }}
            data-testid="editor-keys-toggle"
            onClick={() => setKeysOpen((v) => !v)}
          >
            ? keys
          </button>
          <div class="keys" classList={{ on: keysOpen() }} data-testid="editor-keys">
            <span class="kb">
              <kbd>␣</kbd> <b>play</b>
            </span>
            <span class="kb">
              <kbd>↵</kbd> <b>solo</b>
            </span>
            <span class="kb">
              <kbd>[</kbd>
              <kbd>]</kbd> seg
            </span>
            <span class="kb">
              <kbd>,</kbd>
              <kbd>.</kbd> nudge in
            </span>
            <span class="kb">
              <kbd>s</kbd> speed
            </span>
            <span class="kb">
              <kbd>h</kbd> hold
            </span>
            <span class="kb">
              <kbd>x</kbd> <span style={{ color: "var(--red)" }}>cut</span>
            </span>
            <span class="kb">
              <kbd>e</kbd> <b>export</b>
            </span>
          </div>
          <button
            class="chip"
            classList={{ on: hintsOn() }}
            data-testid="editor-hints-toggle"
            onClick={toggleHints}
          >
            hints {hintsOn() ? "on" : "off"}
          </button>
          <span class="live">
            <span class="rec" classList={{ unsaved: editorStore.isDirty() }} />
            <span data-testid="editor-save-state">
              {editorStore.isDirty() ? "unsaved" : "saved"}
            </span>
          </span>
          <button class="exp-btn u" data-testid="editor-export" onClick={openExportDrawer}>
            Export cut →
          </button>
        </footer>

        <Show when={persistError()}>
          {(msg) => (
            <p class="error persist-line" data-testid="editor-persist-error" role="alert">
              {msg()}
            </p>
          )}
        </Show>

        {/* EXPORT DRAWER */}
        <ExportDrawer
          open={drawerOpen()}
          onClose={closeExportDrawer}
          srcName={srcName()}
          cutLabel={mmss(cutDur())}
          cutDurationSecs={cutDur()}
          persistBusy={persistBusy()}
          onExportCast={doExport}
          onSave={() => void doSave()}
          onToast={toast}
        />

        {/* CONTACT-SHEET LIGHTBOX */}
        <FrameLightbox
          segment={lightbox()?.seg ?? null}
          frame={lightbox()?.frame ?? null}
          cutDurSecs={(() => {
            const seg = lightbox()?.seg;
            if (!seg) return 0;
            return scheduleOf(editorStore.model()).segments.find((s) => s.id === seg.id)?.cutDur ?? 0;
          })()}
          onClose={() => setLightbox(null)}
        />

        {/* QUIT GUARD */}
        <Show when={quitPrompt()}>
          {(prompt) => (
            <div
              class="quit-guard"
              role="dialog"
              aria-modal="true"
              aria-label="Unsaved changes"
              data-testid="editor-quit-guard"
            >
              <div class="quit-box">
                <div class="quit-h">
                  <span class="amber">◆</span>
                  <b class="u">Unsaved changes</b>
                </div>
                <div class="quit-b">
                  Save before {prompt().target === "quit" ? "quitting" : "closing"}?
                </div>
                <div class="quit-f">
                  <button class="go" data-testid="editor-quit-save" disabled={persistBusy()} onClick={onQuitSave}>
                    Save
                  </button>
                  <button class="btn" data-testid="editor-quit-discard" onClick={onQuitDiscard}>
                    Discard
                  </button>
                  <button class="btn" data-testid="editor-quit-cancel" onClick={closeQuitPrompt}>
                    Cancel
                  </button>
                </div>
              </div>
            </div>
          )}
        </Show>

        {/* TOASTS */}
        <div id="toasts" data-testid="editor-toasts">
          <For each={toasts()}>
            {(t) => (
              <div class="toast">
                <span class="g" style={t.color ? { color: t.color } : undefined}>
                  {t.msg[0]}
                </span>
                {t.msg.slice(1)}
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
};
