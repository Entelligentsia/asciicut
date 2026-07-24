import type { Component } from "solid-js";
import { createSignal, Show } from "solid-js";
import { fetchFrame, type FrameDto } from "../lib/api";
import { FrameGrid } from "../components/FrameGrid";
import { PlayerPreview } from "../components/PlayerPreview";

/** Lightweight metadata read from a dropped/opened `.cast` client-side. */
interface DroppedMeta {
  name: string;
  bytes: number;
}

/**
 * Load-cast view (AC#3).
 *
 * Drag-drop + file-open affordances let the user select a `.cast`. Because the
 * T07 server exposes NO arbitrary-cast upload/open endpoint, a dropped file is
 * read CLIENT-SIDE (FileReader) only for validation/metadata display; the frame
 * grid rendered below is the SERVER'S LAUNCH CAST via `GET /api/frame?t=0`. An
 * explicit in-UI notice makes that distinction so the user is never misled into
 * thinking the dropped file's frames are shown for the SERVER grid.
 *
 * The dropped cast IS, however, previewed for real: its text is read
 * client-side and fed IN-MEMORY to `<PlayerPreview>` (T10), so the Load view
 * plays back the actual dropped recording without any server open endpoint.
 * Loading an arbitrary client-selected cast into the server-backed frame grid
 * remains a documented deferral (needs a server upload endpoint or the full
 * wasm-bindgen path).
 */
export const LoadCast: Component = () => {
  const [frame, setFrame] = createSignal<FrameDto | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [dropped, setDropped] = createSignal<DroppedMeta | null>(null);
  const [castText, setCastText] = createSignal<string | null>(null);
  const [dragActive, setDragActive] = createSignal(false);

  async function loadLaunchCast(): Promise<void> {
    setError(null);
    try {
      setFrame(await fetchFrame(0));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  function inspectFile(file: File): void {
    setDropped({ name: file.name, bytes: file.size });
    setCastText(null);
    // Read client-side — never uploaded. The text is used for metadata AND fed
    // in-memory to the player preview (T10).
    const reader = new FileReader();
    reader.onerror = () => setError(`could not read ${file.name}`);
    reader.onload = () => {
      const text = typeof reader.result === "string" ? reader.result : "";
      setCastText(text.length > 0 ? text : null);
    };
    reader.readAsText(file);
    void loadLaunchCast();
  }

  function onDrop(event: DragEvent): void {
    event.preventDefault();
    setDragActive(false);
    const file = event.dataTransfer?.files?.[0];
    if (file) inspectFile(file);
  }

  function onFilePick(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (file) inspectFile(file);
  }

  return (
    <section class="view" data-testid="load-cast">
      <h2>Load a recording</h2>

      <div
        class="dropzone"
        classList={{ "dropzone--active": dragActive() }}
        data-testid="dropzone"
        onDragOver={(e) => {
          e.preventDefault();
          setDragActive(true);
        }}
        onDragLeave={() => setDragActive(false)}
        onDrop={onDrop}
      >
        <p>Drag & drop a <code>.cast</code> file here</p>
        <label class="dropzone__pick">
          or open a file
          <input
            type="file"
            accept=".cast,application/json"
            data-testid="file-input"
            onChange={onFilePick}
          />
        </label>
      </div>

      <button
        type="button"
        class="load-launch"
        data-testid="load-launch"
        onClick={() => void loadLaunchCast()}
      >
        Render server's launch cast
      </button>

      <Show when={dropped()}>
        {(meta) => (
          <p class="notice" data-testid="dropped-notice">
            Selected <strong>{meta().name}</strong> ({meta().bytes} bytes),
            validated locally and previewed below. Note: the{" "}
            <strong>frame grid</strong> further down renders the{" "}
            <strong>server's launch cast</strong>, not this dropped file — the
            server has no arbitrary-cast open endpoint yet.
          </p>
        )}
      </Show>

      <Show when={dropped()}>
        <div class="load-preview" data-testid="load-preview">
          <h3 class="load-preview__title">Recording preview</h3>
          <PlayerPreview data={castText()} />
        </div>
      </Show>

      <Show when={error()}>
        {(msg) => (
          <p class="error" data-testid="load-error" role="alert">
            {msg()}
          </p>
        )}
      </Show>

      <Show when={frame()}>
        {(f) => <FrameGrid frame={f()} />}
      </Show>
    </section>
  );
};
