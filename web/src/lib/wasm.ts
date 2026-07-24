// WASM plumbing proof (AC#1, D3).
//
// asciicut_core.wasm is loaded through Vite's `?init` integration — the init
// function returns an instantiated `WebAssembly.Instance` (typed by vite/client;
// vite-plugin-wasm + top-level-await are also enabled in vite.config.ts to cover
// wasm-with-imports generally). The crate exposes NO wasm-bindgen exports yet
// (its only public item is a `&'static str` accessor, not callable across the
// raw wasm ABI), so this is a *plumbing proof*, not compute: we instantiate the
// module and assert the instance carries `exports`/`memory`, proving the browser
// load path end to end. All functional compute is server-backed (see ./api.ts).
// A follow-up task must add wasm-bindgen frame/compose exports to realise the
// in-browser demo.
import initAsciicutCore from "../wasm/asciicut_core.wasm?init";

export interface WasmSmoke {
  ok: boolean;
  exports: string[];
  hasMemory: boolean;
}

/**
 * Instantiate asciicut_core.wasm and smoke-assert its exports are present.
 * Returns the export names + whether a memory is exported so callers/tests can
 * confirm the module loaded and instantiated.
 */
export async function initWasm(): Promise<WasmSmoke> {
  const instance = await initAsciicutCore();
  const exports = Object.keys(instance.exports);
  const hasMemory = instance.exports.memory instanceof WebAssembly.Memory;
  return { ok: instance instanceof WebAssembly.Instance, exports, hasMemory };
}
