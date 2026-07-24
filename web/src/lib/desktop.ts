// Central hub for desktop-only Tauri events and native menu actions
// (CASTCU-S2-T03). The shell forwards File-menu choices and quit guards as
// Tauri events; this module subscribes to them and dispatches to the editor.
//
// In a browser (no Tauri) all listeners are no-ops, so the same bundle runs
// unchanged against asciicut-server's `/api/*` HTTP surface.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import type { ExportProgressPayload, OpenedCast } from "./api";

/** Target of a blocked close/exit guard. */
export type QuitTarget = "close" | "quit";

/** Payload for the `confirm-quit` event emitted by the shell. */
export interface ConfirmQuitPayload {
  target: QuitTarget;
}

/** Actions the editor provides to handle desktop-native requests. */
export interface DesktopActions {
  /** File → Open: present the OS file picker and load the chosen cast. */
  readonly openCast: () => Promise<void>;
  /** File → Save: persist to the active save path. */
  readonly save: () => Promise<unknown>;
  /** File → Save As: present the OS save dialog and move the active path. */
  readonly saveAs: () => Promise<unknown>;
  /** File → Export: open the export drawer (shell-only today). */
  readonly export: () => void;
  /** File → Quit / window close with unsaved edits: ask and confirm. */
  readonly confirmQuit: (target: QuitTarget) => void;
  /** File → Quit: ask the shell to exit (dirty guard lives in Rust). */
  readonly requestQuit: () => Promise<void>;
  /** A cast was loaded (File → Open, a Recent entry, or the sample); re-hydrate. */
  readonly onCastOpened: (opened: OpenedCast) => void;
}

/** Starts all Tauri event listeners and returns a cleanup function. */
export async function startDesktopBridge(
  actions: DesktopActions,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }

  const unlisteners: UnlistenFn[] = [];

  const on = async <T, >(
    event: string,
    handler: (payload: T) => void,
  ): Promise<void> => {
    const unlisten = await listen<T>(event, (e) => handler(e.payload));
    unlisteners.push(unlisten);
  };

  await on("menu:open", () => {
    void actions.openCast();
  });
  await on("menu:save", () => {
    void actions.save();
  });
  await on("menu:save-as", () => {
    void actions.saveAs();
  });
  await on("menu:export", () => {
    actions.export();
  });
  await on("menu:quit", () => {
    void actions.requestQuit();
  });
  await on("cast-opened", (opened: OpenedCast) => {
    actions.onCastOpened(opened);
  });
  await on("confirm-quit", (payload: ConfirmQuitPayload) => {
    actions.confirmQuit(payload.target);
  });

  return () => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
  };
}

/**
 * Subscribe to the desktop shell's `export-progress` event stream
 * (CASTCU-S2-T05) — the export drawer's own hook, distinct from
 * {@link startDesktopBridge}'s app-lifetime menu/quit-guard listeners because
 * the drawer only needs to listen while a render is in flight. A no-op
 * (never-resolving) subscription in a browser, so callers can `await` +
 * `unlisten()` unconditionally: `unlisten()` on the returned function is
 * always safe, even outside Tauri.
 */
export async function onExportProgress(
  handler: (payload: ExportProgressPayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) {
    return () => {};
  }
  return listen<ExportProgressPayload>("export-progress", (e) => handler(e.payload));
}
