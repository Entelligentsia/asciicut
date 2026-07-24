// The shared reactive edit store — the single source of truth for the editor
// UI *and* the imperative `window.asciicut` command surface (SPEC §8.4).
//
// T11 kept the `EditModel` + `selectedId` as local signals inside `Editor.tsx`.
// To make `window.asciicut` a first-class command API that mutates the SAME state
// the UI renders — independent of component lifecycle — this module lifts that
// state into a module-singleton reactive store built with Solid's `createRoot`
// (an owner root that never disposes, so the signals outlive any view mount).
//
// The store holds `model` / `selectedId` / `source` / `duration` / `isDirty` as
// signals plus command-shaped mutators that are THIN WRAPPERS over the
// already-tested pure `lib/segments` reducers (all clamping invariants proven
// there). `Editor.tsx` becomes a projection over this store; `automation.ts`
// wires the command API to the same mutators. Neither owns the state — both
// share it.

import { isTauri } from "@tauri-apps/api/core";
import { createRoot, createSignal, type Accessor } from "solid-js";
import {
  createSegment,
  emptyModel,
  fromProject,
  setHoldEnd,
  setIdleCap as setModelIdleCap,
  setSpeed as setModelSpeed,
  toProject,
  type EditModel,
  type MarkerDto,
  type OutputDto,
  type ProjectDto,
} from "./segments";
import { setDirty as notifySetDirty } from "./api";

/**
 * The imperative payload for {@link EditorStore.addSegment} (SPEC §8.4's
 * `addSegment` example). `speed` / `hold` / `label` are optional tuning applied
 * to the freshly-appended segment; `label` is a human tag that is projected onto
 * the wire shape (see `toProject`) so it survives `export()`.
 */
export interface AddSegmentInput {
  /** Window start (inclusive) in absolute source seconds. */
  readonly srcStart: number;
  /** Window end (inclusive) in absolute source seconds. */
  readonly srcEnd: number;
  /** Playback speed multiplier (`> 0`); omitted → default speed. */
  readonly speed?: number;
  /** End-hold seconds (`>= 0`); omitted → no hold. */
  readonly hold?: number;
  /** Optional human label, projected into the wire shape. */
  readonly label?: string;
}

/** The shared editor store surface (signals + command-shaped mutators). */
export interface EditorStore {
  /** The immutable edit model (single source of truth for the segment track). */
  readonly model: Accessor<EditModel>;
  /** Replace the whole model (the `onModel` reducer-result sink for the views). */
  readonly setModel: (m: EditModel) => void;
  /** The selected segment id, or `null`. */
  readonly selectedId: Accessor<string | null>;
  /** Set the selection directly (the `onSelect` sink for the views). */
  readonly setSelectedId: (id: string | null) => void;
  /** The project `source` filename (client metadata; set by `open(source)`). */
  readonly source: Accessor<string>;
  /** Record the project source filename. */
  readonly setSource: (s: string) => void;
  /** The known launch-cast duration in seconds (published by the editor view). */
  readonly duration: Accessor<number>;
  /** Publish the canonical duration (clamps new-segment windows). */
  readonly setDuration: (d: number) => void;
  /** Whether the current model has unsaved edits. */
  readonly isDirty: Accessor<boolean>;
  /** Mark the model dirty and notify the desktop shell (Tauri only). */
  readonly markDirty: () => void;
  /** Mark the model clean and notify the desktop shell (Tauri only). */
  readonly markClean: () => void;
  /** Append a segment (SPEC §8.4), applying optional speed/hold/label; returns its id. */
  readonly addSegment: (input: AddSegmentInput) => string;
  /** Select a segment by id (`null` clears the selection). */
  readonly select: (id: string | null) => void;
  /** Set the SELECTED segment's speed; throws if nothing is selected. */
  readonly setSelectedSpeed: (v: number) => void;
  /** Set the SELECTED segment's end-hold; throws if nothing is selected. */
  readonly setSelectedHold: (v: number) => void;
  /** Set the global idle cap (seconds). */
  readonly setIdleCap: (v: number) => void;
  /** The opaque pass-through `output` block, or `null` when absent. */
  readonly output: Accessor<OutputDto | null>;
  /** The opaque pass-through marker list (empty when absent). */
  readonly markers: Accessor<readonly MarkerDto[]>;
  /**
   * Hydrate the whole editor state from a decoded `.asciicut.json` project
   * (the load-on-open path, AC#1): replaces the model, source, and opaque
   * output/markers metadata, clears the selection, and marks the model clean.
   */
  readonly loadProject: (dto: ProjectDto) => void;
  /** Project the in-memory model + opaque metadata onto the wire shape. */
  readonly getProject: () => ProjectDto;
  /** Reset to an empty model + no selection (test / re-open convenience). */
  readonly reset: () => void;
}

function build(): EditorStore {
  const [model, setModelSig] = createSignal<EditModel>(emptyModel());
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [source, setSource] = createSignal<string>("");
  const [duration, setDuration] = createSignal<number>(0);
  const [isDirty, setIsDirty] = createSignal<boolean>(false);
  // Opaque pass-through project metadata (T13): carried verbatim through
  // load→edit→save so `compose()` output geometry + markers survive, WITHOUT
  // polluting the pure `EditModel` (which stays segments + idleCap).
  const [output, setOutput] = createSignal<OutputDto | null>(null);
  const [markers, setMarkers] = createSignal<readonly MarkerDto[]>([]);

  const pushDirty = (dirty: boolean): void => {
    setIsDirty(dirty);
    if (isTauri()) {
      // Fire-and-forget: the shell uses this only for the quit guard, so a
      // delayed or dropped notification is harmless in practice.
      void notifySetDirty(dirty).catch((err: unknown) => {
        console.warn("[asciicut] set_dirty failed", err);
      });
    }
  };

  const markDirty = (): void => {
    pushDirty(true);
  };

  const markClean = (): void => {
    pushDirty(false);
  };

  const setModel = (m: EditModel): void => {
    setModelSig(m);
    markDirty();
  };

  const requireSelected = (): string => {
    const id = selectedId();
    if (id === null) {
      throw new Error("asciicut: no segment selected — call select(id) first");
    }
    return id;
  };

  const addSegment = (input: AddSegmentInput): string => {
    const { model: withSeg, id } = createSegment(
      model(),
      input.srcStart,
      input.srcEnd,
      duration(),
      input.label,
    );
    let next = withSeg;
    if (input.speed !== undefined) next = setModelSpeed(next, id, input.speed);
    if (input.hold !== undefined) next = setHoldEnd(next, id, input.hold);
    setModelSig(next);
    setSelectedId(id);
    markDirty();
    return id;
  };

  const select = (id: string | null): void => {
    setSelectedId(id);
  };

  const setSelectedSpeed = (v: number): void => {
    setModelSig(setModelSpeed(model(), requireSelected(), v));
    markDirty();
  };

  const setSelectedHold = (v: number): void => {
    setModelSig(setHoldEnd(model(), requireSelected(), v));
    markDirty();
  };

  const setIdleCap = (v: number): void => {
    setModelSig(setModelIdleCap(model(), v));
    markDirty();
  };

  const loadProject = (dto: ProjectDto): void => {
    const hydrated = fromProject(dto);
    setModelSig(hydrated.model);
    setSource(hydrated.source);
    setOutput(hydrated.output ?? null);
    setMarkers(hydrated.markers ?? []);
    // Fresh client ids invalidate any prior selection handle.
    setSelectedId(null);
    markClean();
  };

  const getProject = (): ProjectDto => {
    const out = output();
    const mk = markers();
    return toProject(model(), source(), {
      ...(out !== null ? { output: out } : {}),
      ...(mk.length > 0 ? { markers: mk } : {}),
    });
  };

  const reset = (): void => {
    setModelSig(emptyModel());
    setSelectedId(null);
    setOutput(null);
    setMarkers([]);
    markClean();
  };

  return {
    model,
    setModel,
    selectedId,
    setSelectedId,
    source,
    setSource,
    duration,
    setDuration,
    isDirty,
    markDirty,
    markClean,
    addSegment,
    select,
    setSelectedSpeed,
    setSelectedHold,
    setIdleCap,
    output,
    markers,
    loadProject,
    getProject,
    reset,
  };
}

/**
 * The module-singleton editor store. `createRoot` gives the signals an owner
 * that is never disposed, so they persist across editor mounts/unmounts and are
 * safe to read/mutate from `window.asciicut` while the view is off-screen.
 */
export const editorStore: EditorStore = createRoot(build);
