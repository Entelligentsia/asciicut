// Durable, low-stakes UI state: the welcome screen's Recent list plus the two
// "teach once" flags (hints on/off, first-run coach mark seen).
//
// ─── WHY localStorage ────────────────────────────────────────────────────────
// The brief (§3A) requires Recent to survive a restart and to "degrade
// gracefully in browser/server mode". A Tauri state file would satisfy the
// first but needs a Rust round-trip and has no browser equivalent; localStorage
// satisfies both from one code path, because Tauri v2 serves the SPA from a
// FIXED origin (`tauri://localhost`, `http://tauri.localhost` on Windows) whose
// storage persists across launches in WKWebView / WebView2 / WebKitGTK alike.
//
// None of this is project data. A cleared store costs the user a convenience
// list, never an edit — so every read is defensive and every failure is
// swallowed to a safe default rather than surfaced as an error. That is also
// what makes private-mode / storage-disabled webviews a non-event: the app runs
// with an empty Recent list instead of failing to boot.
//
// Side-effect-ful by nature (it touches `localStorage`), but dependency-free
// otherwise: no DOM beyond the storage API, no Solid.

/** One entry in the welcome screen's Recent list. */
export interface RecentEntry {
  /** Absolute path the cast was loaded from — what `open_cast_path` re-opens. */
  readonly path: string;
  /** The file name, for display (`forge_sprint.cast`). */
  readonly name: string;
  /** `Date.now()` at the last open — drives ordering and the "2 days ago". */
  readonly openedAt: number;
  /** The recording's raw duration in seconds, when known (shown as `17:04`). */
  readonly durationSecs?: number;
}

/** How many recordings the Recent list remembers. */
const MAX_RECENT = 8;

const KEY_RECENT = "asciicut.recent";
const KEY_HINTS = "asciicut.hints";
const KEY_COACH = "asciicut.coachSeen";

/**
 * `localStorage`, or `null` where it is unavailable (a webview with storage
 * disabled, a private window, an SSR/test context with no `window`). Every
 * caller below treats `null` as "this session simply has no memory".
 */
function store(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    // Accessing `localStorage` itself throws under some privacy settings.
    return null;
  }
}

function readJson<T>(key: string, fallback: T): T {
  const s = store();
  if (!s) return fallback;
  try {
    const raw = s.getItem(key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown): void {
  const s = store();
  if (!s) return;
  try {
    s.setItem(key, JSON.stringify(value));
  } catch {
    // Quota exceeded / storage disabled mid-session — a lost convenience list.
  }
}

/** Whether `value` is a usable {@link RecentEntry} (guards a hand-edited store). */
function isEntry(value: unknown): value is RecentEntry {
  if (typeof value !== "object" || value === null) return false;
  const e = value as Partial<RecentEntry>;
  return (
    typeof e.path === "string" &&
    e.path.length > 0 &&
    typeof e.name === "string" &&
    typeof e.openedAt === "number"
  );
}

/** The Recent list, newest first. Always a valid array, never throws. */
export function recentCasts(): RecentEntry[] {
  const raw = readJson<unknown>(KEY_RECENT, []);
  if (!Array.isArray(raw)) return [];
  return raw
    .filter(isEntry)
    .sort((a, b) => b.openedAt - a.openedAt)
    .slice(0, MAX_RECENT);
}

/**
 * Record (or refresh) a recording in the Recent list, moving it to the front.
 * De-duplicates by `path`, so re-opening the same file promotes its entry
 * instead of growing the list.
 */
export function rememberCast(
  path: string,
  name: string,
  durationSecs?: number,
): void {
  if (!path) return;
  const entry: RecentEntry = {
    path,
    name,
    openedAt: Date.now(),
    ...(durationSecs !== undefined && durationSecs > 0 ? { durationSecs } : {}),
  };
  const next = [entry, ...recentCasts().filter((e) => e.path !== path)].slice(
    0,
    MAX_RECENT,
  );
  writeJson(KEY_RECENT, next);
}

/**
 * Drop a recording from the Recent list — what the welcome screen does when
 * re-opening one fails because the file moved or was deleted, so a dead entry
 * is not offered twice.
 */
export function forgetCast(path: string): void {
  writeJson(
    KEY_RECENT,
    recentCasts().filter((e) => e.path !== path),
  );
}

/** Whether teaching copy is shown. Defaults to on (a first-run user needs it). */
export function hintsEnabled(): boolean {
  return readJson<boolean>(KEY_HINTS, true) !== false;
}

/** Persist the `hints on|off` footer toggle. */
export function setHintsEnabled(on: boolean): void {
  writeJson(KEY_HINTS, on);
}

/** Whether the first-run coach mark has already been dismissed. */
export function coachSeen(): boolean {
  return readJson<boolean>(KEY_COACH, false) === true;
}

/** Record that the coach mark was dismissed, so it never returns. */
export function markCoachSeen(): void {
  writeJson(KEY_COACH, true);
}

/**
 * A coarse "2 days ago" for a Recent entry's timestamp. Deliberately coarse:
 * the list is for recognition, and an exact clock time would be noise.
 */
export function relativeTime(at: number, now: number = Date.now()): string {
  const secs = Math.max(0, (now - at) / 1000);
  if (secs < 90) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return hours === 1 ? "an hour ago" : `${hours} hours ago`;
  const days = Math.round(hours / 24);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  const weeks = Math.round(days / 7);
  return weeks === 1 ? "last week" : `${weeks} weeks ago`;
}
