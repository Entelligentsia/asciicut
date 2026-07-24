/* @refresh reload */
import { render } from "solid-js/web";
import { App } from "./App";
import { installAutomation } from "./lib/automation";
import { initWasm } from "./lib/wasm";
// Vendored asciinema-player v3 stylesheet (T10) — bundled by Vite alongside the
// SPA styles so the in-memory terminal preview renders correctly.
import "asciinema-player/dist/bundle/asciinema-player.css";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("#root mount point missing from index.html");
}

const automation = installAutomation(
  (import.meta.env.VITE_APP_VERSION as string | undefined) ?? "0.1.0",
);

render(() => <App />, root);
automation.ready = true;

// Kick the WASM plumbing proof (AC#1). Non-fatal: the SPA is server-backed, so
// a wasm load failure is logged but does not block the UI.
void initWasm()
  .then((smoke) =>
    console.info(
      `[asciicut] wasm plumbing ok=${smoke.ok} exports=[${smoke.exports.join(", ")}]`,
    ),
  )
  .catch((err) => console.error("[asciicut] wasm init failed", err));
