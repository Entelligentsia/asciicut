// The editor's time vocabulary, in one place.
//
// The v2 refactor's rule is that a duration and a timestamp never render in
// different units side by side (brief §3C: an earlier revision leaked raw
// seconds `618.00` next to `10:18`). Every surface that prints a source time —
// the header ratio, the IN/OUT marks, the scrub chip, the contact-sheet meta —
// imports from here rather than carrying its own formatter, so "one time
// format" is enforced by there being exactly one implementation.
//
// Pure + side-effect-free (no DOM, no Solid).

/**
 * `m:ss` — the coarse form, for durations and whole-second timestamps
 * (`17:04`, `0:36`). Rounds to the nearest second; negatives clamp to `0:00`.
 */
export function mmss(secs: number): string {
  const n = Math.max(0, Math.round(Number.isFinite(secs) ? secs : 0));
  return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, "0")}`;
}

/**
 * `m:ss.ss` — the precise form, for a boundary the user is nudging onto a
 * specific event (`10:18.00`). Same unit as {@link mmss}, two extra decimals;
 * NEVER a bare seconds count, which is the format bug §3C calls out.
 */
export function mmssx(secs: number): string {
  const s = Math.max(0, Number.isFinite(secs) ? secs : 0);
  const m = Math.floor(s / 60);
  return `${m}:${(s - m * 60).toFixed(2).padStart(5, "0")}`;
}

/**
 * The raw→cut trim percentage as a display string (`96% trimmed`), or `—` when
 * there is no raw duration to compare against. Shared by the header hero and
 * the export drawer so the headline number cannot drift between them.
 */
export function trimmedPct(rawSecs: number, cutSecs: number): string {
  if (!(rawSecs > 0)) return "—";
  return `${Math.round((1 - cutSecs / rawSecs) * 100)}% trimmed`;
}
