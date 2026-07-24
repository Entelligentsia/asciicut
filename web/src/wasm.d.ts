// Ambient module typing for bare `*.wasm` imports.
//
// The active WASM plumbing proof imports `asciicut_core.wasm?init` (typed by
// `vite/client` as `(imports?) => Promise<WebAssembly.Instance>`). This file
// covers the *bare* `*.wasm` form that vite-plugin-wasm's ESM integration
// resolves (re-exporting the module's own exports), so `tsc --noEmit` (AC#4)
// stays green regardless of which import style a later task uses. The exports
// shape is intentionally open (`Record<string, unknown>`) because asciicut_core
// exposes no wasm-bindgen exports yet (D3 — plumbing proof, not compute).
declare module "*.wasm" {
  const wasmModule: Record<string, unknown>;
  export default wasmModule;
}
