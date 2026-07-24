# Compose golden fixtures

These are the reference outputs the Rust `compose` engine (`asciicut-core`) is
diffed against. They are the **prototype's own output** (`prototype/compose.py`,
the AC-named source of truth for CASTCU-S1-T05), captured test-first before the
Rust port was written.

## Files

- `sample.composed.cast` — the composed output for the real 17-minute recording
  `samples/sample.cast` driven by `samples/sample.asciicut.json`. Regenerate from
  the repo root with the exact command:

  ```sh
  python3 prototype/compose.py samples/sample.asciicut.json \
      > crates/asciicut-core/tests/fixtures/compose/sample.composed.cast
  ```

- `synthetic.cast` + `synthetic.asciicut.json` — a tiny hand-computable source +
  project whose composed timings are asserted **exactly** in `tests/compose.rs`
  (independent of the large sample). The expected composed stream is:

  ```
  [0.0, "o", "a"] [0.2, "o", "b"] [0.6, "o", "c"] [1.1, "o", "d"] [2.1, "o", ""]
  ```

  Regenerate/verify with:

  ```sh
  python3 prototype/compose.py \
      crates/asciicut-core/tests/fixtures/compose/synthetic.asciicut.json
  ```

## Comparison discipline

The golden test compares **semantically**, not byte-for-byte: it re-parses both
the Rust output and the reference fixture and asserts header equality, identical
event count, byte-exact `code`/`data` payloads, and event times equal within a
small epsilon (`1e-6`). This avoids brittleness in float text formatting (Python
`round()` is round-half-to-even; JSON float repr differs between runtimes) while
still pinning the compose contract. Native ↔ wasm byte-identity (SPEC §7.1) is
guaranteed separately by the single shared pure Rust implementation.
