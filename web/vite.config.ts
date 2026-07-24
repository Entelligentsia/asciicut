import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// Vite config for the asciicut SPA (CASTCU-S1-T08).
//
// - `wasm` + `topLevelAwait`: let `import init from "*.wasm"` resolve through
//   vite-plugin-wasm's ESM integration (the WASM plumbing proof, D3/AC#1).
// Two build targets, selected by Vite `mode` (CASTCU-S2-T01, PLAN D3):
// - default (server) `mode`: `build.outDir` targets the server's rust-embed
//   folder so `vite build` feeds the S1 `#[folder = "web/"]` embed loop directly
//   (D2). `emptyOutDir: false`: NEVER delete the tracked placeholder index.html;
//   the build overwrites it, and the implement/commit flow restores the
//   committed baseline afterward. The generated `assets/` are gitignored.
// - `--mode desktop`: emits a CLEAN, dedicated `web/dist/` (gitignored) that
//   Tauri bundles as its `frontendDist`. This decouples the desktop bundle from
//   the server's embed directory and its commit-hygiene dance; the server build
//   path is left entirely unchanged.
export default defineConfig(({ mode }) => {
  const desktop = mode === "desktop";
  return {
    plugins: [solid(), wasm(), topLevelAwait()],
    // Dev-only: the SPA calls relative `/api/*`, which the native asciicut-server
    // answers. Proxy those to the running server so `vite dev` (HMR) previews
    // against real activity/thumbs/compose data. No effect on the production
    // build (the server embeds the SPA and serves both from one origin; the
    // desktop bundle serves the SPA chrome — the no-listener bridge is T02).
    server: {
      proxy: {
        "/api": "http://127.0.0.1:8777",
      },
    },
    build: desktop
      ? { outDir: "dist", emptyOutDir: true }
      : { outDir: "../crates/asciicut-server/web", emptyOutDir: false },
  };
});
