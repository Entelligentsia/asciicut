/**
 * The fixed inter-segment beat (seconds) — mirrors `asciicut-core::compose::BEAT`
 * (`0.5`). Charged before every non-first segment in the compose engine, so the
 * approximate JS schedule must charge the same beat to stay aligned with the
 * real composed `.cast` the player plays. Single source of truth for both the
 * schedule helper and any UI that needs the value.
 */
export const BEAT = 0.5;