//! Activity signal — the per-bucket change-density waveform of a [`Cast`].
//!
//! This is `asciicut-core`'s third pure primitive, beside the [`cast`](crate::cast)
//! parser and the [`frame`](crate::frame) engine. It reduces the normalized event
//! stream into a compact array of per-time-bucket **change-density** scores: how
//! much visible terminal change each ~0.25s slice of the recording carries. The
//! filmstrip/scrubbing UI (T09) reads this waveform to find dead air versus busy
//! regions without replaying the whole terminal.
//!
//! ## Purity / parity discipline (SPEC §7.1)
//!
//! Like the parser and frame engine, this module is **pure**: model in, array
//! out. It performs no `std::fs`, time, thread, or process I/O, so it compiles
//! byte-identically to `wasm32-unknown-unknown` and preserves the native ↔ wasm
//! parity contract. It consumes the already-parsed [`Cast`] model and never
//! re-parses raw `.cast` text.
//!
//! ## Scoring: printable / cursor-affecting bytes (SPEC §4.5)
//!
//! The score for a bucket is the count of **output** bytes in that slice that
//! change what a viewer perceives — printable glyph bytes plus cursor/line
//! control bytes (newline, carriage-return, backspace, tab, and cursor-movement
//! or erase CSI) — while **skipping pure-styling noise**: SGR colour sequences
//! (`CSI … m`) and OSC runs (`ESC ] … BEL/ST`, e.g. hyperlinks and title sets),
//! which dominate a real recording and would otherwise drown the signal. Only
//! [`EventCode::Output`] events contribute; resize/marker/other events never do.
//!
//! The scanner is a conservative ANSI walker (recognise the `ESC [` CSI and
//! `ESC ]` OSC envelopes, count everything else), scoped to "cheap and good
//! enough for a waveform" — not a full VT. The VT-exact path is grid-diff, which
//! is deliberately left as an inert stub here (see [`apply_grid_diff_weight`]).
//!
//! ## Known limitation (waveform noise, by design)
//!
//! The scanner runs per output payload. An escape sequence split across two
//! `Output` events (the second event's payload starting mid-sequence) is scored
//! independently and may be miscounted. This is accepted waveform noise: the
//! dominant signal is printable glyph bytes, and a byte or two of misattributed
//! control on a boundary does not change which buckets read as busy vs idle.

use crate::cast::{Cast, EventCode};

/// The default bucket duration in seconds (SPEC §4.5). The timeline UI and this
/// probe share one default so the waveform aligns to the scrubber without
/// re-deriving arithmetic.
pub const DEFAULT_BUCKET_SECS: f64 = 0.25;

/// The change-density waveform of a [`Cast`]: the bucket duration plus an ordered
/// per-bucket score array.
///
/// Bucket `i` covers source time `[i * bucket_secs, (i + 1) * bucket_secs)`. Each
/// score is an integer byte count, so equality is exact — the only `f64` field is
/// `bucket_secs`, which is a caller-fixed parameter (never a computed result),
/// keeping [`PartialEq`] sound in line with the [`Frame`](crate::frame) no-float-eq
/// discipline.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySignal {
    bucket_secs: f64,
    buckets: Vec<u64>,
}

impl ActivitySignal {
    /// The ordered per-bucket change-density scores (the waveform array).
    #[must_use]
    pub fn buckets(&self) -> &[u64] {
        &self.buckets
    }

    /// The bucket duration in seconds every index is measured against.
    #[must_use]
    pub fn bucket_secs(&self) -> f64 {
        self.bucket_secs
    }

    /// The number of buckets in the waveform.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    /// Whether the waveform is empty (an event-less recording).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// The source start time (seconds) of bucket `index`.
    ///
    /// A pure convenience so T09 can map a waveform index back to the timeline
    /// without recomputing `index * bucket_secs`. `index` is not bounds-checked
    /// against [`len`](Self::len) — it is plain arithmetic.
    #[must_use]
    pub fn start_time(&self, index: usize) -> f64 {
        // `index` is a byte-bucket ordinal; the widening cast is exact for any
        // realistic recording length.
        index as f64 * self.bucket_secs
    }
}

/// Build the change-density [`ActivitySignal`] for `cast` at `bucket_secs`.
///
/// The recording duration is taken from the last (monotonic, per the parser)
/// event time; the bucket count is `ceil(duration / bucket_secs)`, floored at one
/// bucket for any non-empty recording. Each [`EventCode::Output`] event's score
/// is added to the bucket its absolute [`Event::time`](crate::cast::Event::time)
/// falls into (`floor(time / bucket_secs)`), clamped to the last bucket so an
/// event landing exactly on the final boundary cannot index out of range.
///
/// `bucket_secs` is untrusted: a non-finite or non-positive value falls back to
/// [`DEFAULT_BUCKET_SECS`] rather than panicking or producing a degenerate array.
///
/// # Examples
///
/// ```
/// let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n[0.0, \"o\", \"hi\"]\n";
/// let cast = asciicut_core::Cast::parse(src).unwrap();
/// let signal = asciicut_core::activity_signal(&cast, asciicut_core::DEFAULT_BUCKET_SECS);
/// assert_eq!(signal.buckets(), &[2]); // two printable bytes in one bucket
/// ```
#[must_use]
pub fn activity_signal(cast: &Cast, bucket_secs: f64) -> ActivitySignal {
    // Untrusted input: reject non-positive / NaN / infinite bucket sizes.
    let bucket_secs = if bucket_secs.is_finite() && bucket_secs > 0.0 {
        bucket_secs
    } else {
        DEFAULT_BUCKET_SECS
    };

    // An event-less recording has no waveform.
    if cast.events.is_empty() {
        return ActivitySignal {
            bucket_secs,
            buckets: Vec::new(),
        };
    }

    // Duration = last (monotonic) event time. A single-instant recording (all
    // events at t=0) still yields one bucket so its bytes have somewhere to land.
    let duration = cast.events.last().map_or(0.0, |e| e.time);
    let count = bucket_count(duration, bucket_secs);
    let mut buckets = vec![0u64; count];

    let last = count - 1;
    for event in &cast.events {
        if event.code != EventCode::Output {
            continue;
        }
        // floor(time / bucket) is the bucket index; clamp to the last bucket so an
        // event exactly on the final boundary (ceil-count vs floor-index) cannot
        // over-run. Non-finite/negative times collapse to bucket 0 defensively.
        let raw = (event.time / bucket_secs).floor();
        let idx = if raw.is_finite() && raw >= 0.0 {
            (raw as usize).min(last)
        } else {
            0
        };
        let score = apply_grid_diff_weight(score_payload(event.data.as_bytes()));
        buckets[idx] = buckets[idx].saturating_add(score);
    }

    ActivitySignal {
        bucket_secs,
        buckets,
    }
}

/// The number of buckets spanning `duration` at `bucket_secs`, floored at 1.
///
/// `bucket_secs` is already validated positive/finite by the caller. The
/// ceil-to-`usize` cast is saturated so a hostile duration cannot wrap.
fn bucket_count(duration: f64, bucket_secs: f64) -> usize {
    let raw = (duration / bucket_secs).ceil();
    if raw.is_finite() && raw >= 1.0 {
        // Saturate: clamp above `usize::MAX` before the cast (untrusted input).
        if raw >= usize::MAX as f64 {
            usize::MAX
        } else {
            raw as usize
        }
    } else {
        // duration == 0 (single-instant) or any degenerate value → one bucket.
        1
    }
}

/// Grid-diff refinement seam — **inert stub** (T04 AC#3).
///
/// Returns `score` unchanged. The cheap first pass scores a bucket by its
/// printable/cursor-affecting byte count. The planned refinement (a future task)
/// will multiply by a real per-bucket *grid-change ratio* — replaying `avt` so a
/// spinner or clock that reprints identical cells scores near-zero instead of
/// high — which is out of scope here. Keeping the seam as an `f64` weight of
/// exactly `1.0` lets that land without a signature change; with a unit weight
/// this is an exact identity for byte counts, so no behaviour depends on it.
#[inline]
fn apply_grid_diff_weight(score: u64) -> u64 {
    // grid-diff refinement: T-future. Unit weight ⇒ identity for now.
    let weight = 1.0_f64;
    (score as f64 * weight) as u64
}

/// Count printable / cursor-affecting bytes in one output payload, skipping SGR
/// colour and OSC styling runs.
///
/// A single forward pass recognises the two ANSI envelopes that carry pure
/// styling — `ESC [` CSI (styling only when the final byte is `m`) and `ESC ]`
/// OSC (always styling: hyperlinks, titles) — and counts everything else that
/// moves the cursor or paints a glyph.
fn score_payload(bytes: &[u8]) -> u64 {
    const ESC: u8 = 0x1b;
    let mut score = 0u64;
    let mut i = 0usize;
    let len = bytes.len();

    while i < len {
        let b = bytes[i];
        if b == ESC {
            match bytes.get(i + 1) {
                // CSI: ESC [ … <final 0x40..=0x7e>
                Some(&b'[') => {
                    let mut j = i + 2;
                    while j < len && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < len {
                        // Colour (SGR, final `m`) is styling noise → 0; a
                        // cursor-movement / erase CSI counts as one change.
                        if is_cursor_affecting_csi(bytes[j]) {
                            score += 1;
                        }
                        i = j + 1;
                    } else {
                        // Unterminated CSI: stop — the rest is not renderable.
                        break;
                    }
                }
                // OSC: ESC ] … terminated by BEL (0x07) or ST (ESC \). All styling.
                Some(&b']') => {
                    let mut j = i + 2;
                    while j < len {
                        if bytes[j] == 0x07 {
                            j += 1;
                            break;
                        }
                        if bytes[j] == ESC && bytes.get(j + 1) == Some(&b'\\') {
                            j += 2;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                }
                // Any other ESC sequence (charset select, keypad mode, …): a
                // two-byte control that paints nothing. Skip both bytes.
                Some(_) => i += 2,
                // Lone trailing ESC.
                None => break,
            }
        } else if b < 0x20 || b == 0x7f {
            // C0 control / DEL. Only cursor/line-affecting ones count.
            if matches!(b, 0x08 | 0x09 | 0x0a | 0x0d) {
                score += 1; // BS, TAB, LF, CR
            }
            i += 1;
        } else {
            // Printable byte (UTF-8 lead/continuation bytes included: this is a
            // byte count, per SPEC §4.5).
            score += 1;
            i += 1;
        }
    }

    score
}

/// Whether a CSI final byte moves the cursor or erases — i.e. changes the screen
/// rather than merely restyling it. SGR colour (`m`) and mode toggles are not
/// counted.
fn is_cursor_affecting_csi(final_byte: u8) -> bool {
    matches!(
        final_byte,
        // CUU/CUD/CUF/CUB/CNL/CPL/CHA/CUP  |  ED/EL  |  SU/SD  |  HVP/VPA
        b'A' | b'B'
            | b'C'
            | b'D'
            | b'E'
            | b'F'
            | b'G'
            | b'H'
            | b'J'
            | b'K'
            | b'S'
            | b'T'
            | b'f'
            | b'd'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::{Event, EventCode, Header};

    /// Build a minimal in-memory [`Cast`] from `(time, code, data)` triples — no
    /// I/O, no parsing, so the unit matrix stays pure.
    fn cast_of(events: &[(f64, EventCode, &str)]) -> Cast {
        Cast {
            header: Header {
                version: 2,
                width: 80,
                height: 24,
                timestamp: None,
                theme: None,
                env: None,
            },
            events: events
                .iter()
                .map(|(time, code, data)| Event {
                    time: *time,
                    code: code.clone(),
                    data: (*data).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn empty_stream_yields_empty_waveform() {
        let signal = activity_signal(&cast_of(&[]), DEFAULT_BUCKET_SECS);
        assert!(signal.is_empty());
        assert_eq!(signal.len(), 0);
        assert_eq!(signal.buckets(), &[] as &[u64]);
    }

    #[test]
    fn single_output_at_zero_is_one_bucket_of_exact_byte_count() {
        let signal = activity_signal(
            &cast_of(&[(0.0, EventCode::Output, "hello")]),
            DEFAULT_BUCKET_SECS,
        );
        // Single-instant recording → exactly one bucket; "hello" = 5 printable bytes.
        assert_eq!(signal.buckets(), &[5]);
        assert_eq!(signal.len(), 1);
    }

    #[test]
    fn events_land_in_the_correct_bucket_index() {
        // Buckets are 0.25s: t=0.0 → 0, t=0.30 → 1, t=0.80 → 3.
        let signal = activity_signal(
            &cast_of(&[
                (0.0, EventCode::Output, "aa"),   // bucket 0
                (0.30, EventCode::Output, "bbb"), // bucket 1
                (0.80, EventCode::Output, "c"),   // bucket 3
            ]),
            DEFAULT_BUCKET_SECS,
        );
        assert_eq!(signal.buckets(), &[2, 3, 0, 1]);
        assert!((signal.start_time(3) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn pure_styling_escapes_score_near_zero() {
        // An SGR colour set/reset plus an OSC-8 hyperlink envelope: no glyphs.
        let payload = "\u{1b}[38;5;42m\u{1b}[0m\u{1b}]8;;https://example.com\u{7}\u{1b}]8;;\u{7}";
        let signal = activity_signal(
            &cast_of(&[(0.0, EventCode::Output, payload)]),
            DEFAULT_BUCKET_SECS,
        );
        assert_eq!(signal.buckets(), &[0]);
    }

    #[test]
    fn mixed_printable_and_escapes_counts_only_visible_bytes() {
        // "hi" (2) + colour (0) + "X" (1) + reset (0) + newline (1) = 4.
        let payload = "hi\u{1b}[31mX\u{1b}[0m\n";
        let signal = activity_signal(
            &cast_of(&[(0.0, EventCode::Output, payload)]),
            DEFAULT_BUCKET_SECS,
        );
        assert_eq!(signal.buckets(), &[4]);
    }

    #[test]
    fn cursor_movement_csi_counts_but_sgr_does_not() {
        // CUP (H) + EL (K) count as 2; SGR (m) counts 0.
        let payload = "\u{1b}[2;5H\u{1b}[K\u{1b}[0m";
        let signal = activity_signal(
            &cast_of(&[(0.0, EventCode::Output, payload)]),
            DEFAULT_BUCKET_SECS,
        );
        assert_eq!(signal.buckets(), &[2]);
    }

    #[test]
    fn resize_and_marker_events_contribute_nothing() {
        let signal = activity_signal(
            &cast_of(&[
                (0.0, EventCode::Resize, "80x24"),
                (0.0, EventCode::Marker, "chapter one"),
                (0.0, EventCode::Other("i".to_owned()), "keystrokes"),
            ]),
            DEFAULT_BUCKET_SECS,
        );
        // Non-output events never score, but the recording still has one bucket.
        assert_eq!(signal.buckets(), &[0]);
    }

    #[test]
    fn non_positive_or_nan_bucket_secs_falls_back_to_default() {
        let cast = cast_of(&[(0.0, EventCode::Output, "abc")]);
        for bad in [0.0, -0.25, f64::NAN, f64::INFINITY] {
            let signal = activity_signal(&cast, bad);
            assert!((signal.bucket_secs() - DEFAULT_BUCKET_SECS).abs() < 1e-12);
            assert_eq!(signal.buckets(), &[3]);
        }
    }

    #[test]
    fn event_on_final_bucket_boundary_does_not_panic_or_overrun() {
        // duration = 0.5 → ceil(0.5/0.25) = 2 buckets (indices 0,1). An event at
        // exactly t=0.5 floors to index 2 and must clamp back to the last bucket.
        let signal = activity_signal(
            &cast_of(&[
                (0.0, EventCode::Output, "a"),
                (0.5, EventCode::Output, "bb"),
            ]),
            DEFAULT_BUCKET_SECS,
        );
        assert_eq!(signal.len(), 2);
        // The boundary event's 2 bytes land in the last bucket, not out of range.
        assert_eq!(signal.buckets(), &[1, 2]);
    }

    #[test]
    fn grid_diff_stub_is_inert() {
        // The stub weight is exactly 1.0, so the score equals the raw byte count.
        assert_eq!(apply_grid_diff_weight(0), 0);
        assert_eq!(apply_grid_diff_weight(7), 7);
        assert_eq!(
            apply_grid_diff_weight(u64::from(u32::MAX)),
            u64::from(u32::MAX)
        );
    }
}
