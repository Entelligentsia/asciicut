//! Compose engine — projects an edit project onto an immutable source `.cast`.
//!
//! This is the Rust port of `prototype/compose.py` (the AC-named reference; see
//! SPEC §4.2/§4.3). It consumes the already version-normalized [`Cast`] model
//! produced by the [`cast`](crate::cast) parser and re-projects it through an
//! edit [`Project`]: it keeps the chosen source windows in order, rebases and
//! scales inter-event gaps per segment `speed`, enforces a global `idleCap`,
//! freezes the last frame for `holdEnd` seconds, and inserts a fixed inter-segment
//! beat. The result is a fresh [`Cast`] whose events carry **absolute** seconds,
//! ready to be rendered back to a v2 `.cast` document via
//! [`Cast::to_cast_string`](crate::cast::Cast::to_cast_string).
//!
//! Like the parser and frame engine, the whole surface is **pure**: no
//! `std::fs`, time, thread, or process I/O. Decoding a project is string-in
//! ([`Project::parse`]); reading the file from disk is the caller's job. This
//! keeps the byte-identical native ↔ `wasm32-unknown-unknown` parity contract
//! (SPEC §7.1) — the same math runs identically on both targets, and this module
//! is deliberately independent of `avt`/frame-at-T (it is pure event-stream
//! arithmetic).
//!
//! ## Decode-time numeric validation
//!
//! [`Project::parse`] is **fallible**. After serde decode it rejects, with a
//! typed [`ProjectError`], any project whose numbers would drive non-finite or
//! reversed time into the engine: `speed` must be finite and `> 0` (absent/`0.0`
//! is coerced to `1.0`, matching the prototype's `float(...) or 1.0`; `NaN`,
//! `±inf`, or negative → error), `idleCap` must be finite and `>= 0`, and every
//! `srcStart`/`srcEnd`/`holdEnd` must be finite. With validated finite inputs the
//! compose arithmetic can only emit finite times, so the serializer (which uses
//! `serde_json`, which *errors* on `NaN`/`Infinity`) stays total. Full semantic
//! validation (`srcStart <= srcEnd`, overlap) is intentionally out of scope here —
//! AC#3 is "port before extending", and the prototype tolerates those.

use std::fmt;

use serde::Deserialize;

use crate::cast::{Cast, Event, EventCode};

/// The fixed inter-segment pause (seconds) inserted before every non-first
/// non-empty segment — the prototype's `BEAT`.
const BEAT: f64 = 0.5;

/// The default global idle cap (seconds) when the project omits `idleCap`.
const DEFAULT_IDLE_CAP: f64 = 0.4;

/// An edit project (SPEC §4.2): which source, the output terminal, the global
/// idle cap, and the ordered keep-segments (plus forward-compat `markers`).
///
/// Decoded from `.asciicut.json` text by [`Project::parse`], which also validates
/// and coerces the numeric fields (see the module docs). `output.theme` /
/// `output.fontSize` and segment `id`/`label` and `markers` are accepted for
/// forward-compat but not yet projected into output — exactly as the prototype
/// ignores them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// The source `.cast` filename, relative to the project file (caller resolves).
    pub source: String,
    /// The output terminal block; `None` falls back to the source header.
    #[serde(default)]
    pub output: Option<Output>,
    /// Global idle cap in seconds (defaults to `0.4`).
    #[serde(default = "default_idle_cap")]
    pub idle_cap: f64,
    /// The ordered keep-segments.
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Forward-compat marker list; not yet projected into output.
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// The `output` block of a project — the composed terminal geometry (and the
/// currently-unused `theme`/`fontSize`, kept optional for forward-compat).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Composed terminal width in columns (overrides the source header).
    #[serde(default)]
    pub width: Option<u16>,
    /// Composed terminal height in rows (overrides the source header).
    #[serde(default)]
    pub height: Option<u16>,
    /// Theme hint, accepted but not yet injected into the composed header.
    #[serde(default)]
    pub theme: Option<serde_json::Value>,
    /// Font-size hint, accepted but not yet injected into the composed header.
    #[serde(default)]
    pub font_size: Option<f64>,
}

/// One keep-segment: an inclusive source-time window plus its playback controls.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Window start (inclusive) in absolute source seconds.
    pub src_start: f64,
    /// Window end (inclusive) in absolute source seconds.
    pub src_end: f64,
    /// Playback speed multiplier (inter-event gaps divided by this). Defaults to
    /// `1.0`; a `0.0`/absent value is coerced to `1.0` at parse time.
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// End-hold: freeze the last frame this many seconds after the window.
    #[serde(default)]
    pub hold_end: f64,
    /// Optional stable id (forward-compat; not projected).
    #[serde(default)]
    pub id: Option<String>,
    /// Optional human label (forward-compat; not projected).
    #[serde(default)]
    pub label: Option<String>,
}

/// A forward-compat marker; accepted but not yet projected into output.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Marker {
    /// Source time (seconds) of the marker.
    pub t: f64,
    /// Optional marker text.
    #[serde(default)]
    pub text: Option<String>,
}

fn default_idle_cap() -> f64 {
    DEFAULT_IDLE_CAP
}

fn default_speed() -> f64 {
    1.0
}

/// Everything that can go wrong while decoding/validating a `.asciicut.json`.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    /// The project text was not a valid JSON project object (decoder message).
    InvalidJson(String),
    /// `idleCap` was not finite (`NaN`/`±inf`) — carries the offending value.
    NonFiniteIdleCap(f64),
    /// `idleCap` was finite but negative — carries the offending value.
    NegativeIdleCap(f64),
    /// A segment's `speed` was not finite — carries the 0-based index and value.
    NonFiniteSpeed { segment: usize, value: f64 },
    /// A segment's `speed` was finite but negative — carries index and value.
    NegativeSpeed { segment: usize, value: f64 },
    /// A segment time field (`srcStart`/`srcEnd`/`holdEnd`) was not finite.
    NonFiniteTime {
        /// 0-based segment index.
        segment: usize,
        /// Which field was offending.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::InvalidJson(msg) => write!(f, "invalid project JSON: {msg}"),
            ProjectError::NonFiniteIdleCap(v) => {
                write!(f, "idleCap must be finite, got {v}")
            }
            ProjectError::NegativeIdleCap(v) => {
                write!(f, "idleCap must be >= 0, got {v}")
            }
            ProjectError::NonFiniteSpeed { segment, value } => {
                write!(f, "segment {segment}: speed must be finite, got {value}")
            }
            ProjectError::NegativeSpeed { segment, value } => {
                write!(f, "segment {segment}: speed must be >= 0, got {value}")
            }
            ProjectError::NonFiniteTime {
                segment,
                field,
                value,
            } => write!(f, "segment {segment}: {field} must be finite, got {value}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl Project {
    /// Decode and validate a `.asciicut.json` edit project from text.
    ///
    /// Runs serde decode, then validates/coerces the numeric fields: `idleCap`
    /// must be finite and `>= 0`; each segment's `srcStart`/`srcEnd`/`holdEnd`
    /// must be finite; `speed` must be finite and `>= 0`, with `0.0` coerced to
    /// `1.0` (matching the prototype). Reading the file from disk is the caller's
    /// job — this is a pure string-in decoder.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] for invalid JSON, a non-finite/negative `idleCap`,
    /// a non-finite/negative segment `speed`, or a non-finite segment time.
    ///
    /// # Examples
    ///
    /// ```
    /// let json = r#"{"source":"a.cast","segments":[
    ///     {"srcStart":0.0,"srcEnd":1.0}]}"#;
    /// let project = asciicut_core::Project::parse(json).unwrap();
    /// assert_eq!(project.source, "a.cast");
    /// // `idleCap` and `speed` default when omitted.
    /// assert!((project.idle_cap - 0.4).abs() < 1e-9);
    /// assert!((project.segments[0].speed - 1.0).abs() < 1e-9);
    /// ```
    pub fn parse(input: &str) -> Result<Project, ProjectError> {
        let mut project: Project =
            serde_json::from_str(input).map_err(|e| ProjectError::InvalidJson(e.to_string()))?;

        if !project.idle_cap.is_finite() {
            return Err(ProjectError::NonFiniteIdleCap(project.idle_cap));
        }
        if project.idle_cap < 0.0 {
            return Err(ProjectError::NegativeIdleCap(project.idle_cap));
        }

        for (i, seg) in project.segments.iter_mut().enumerate() {
            for (field, value) in [
                ("srcStart", seg.src_start),
                ("srcEnd", seg.src_end),
                ("holdEnd", seg.hold_end),
            ] {
                if !value.is_finite() {
                    return Err(ProjectError::NonFiniteTime {
                        segment: i,
                        field,
                        value,
                    });
                }
            }

            if !seg.speed.is_finite() {
                return Err(ProjectError::NonFiniteSpeed {
                    segment: i,
                    value: seg.speed,
                });
            }
            if seg.speed < 0.0 {
                return Err(ProjectError::NegativeSpeed {
                    segment: i,
                    value: seg.speed,
                });
            }
            // Prototype: `float(seg.get("speed", 1.0)) or 1.0` — 0.0 is falsy.
            if seg.speed == 0.0 {
                seg.speed = 1.0;
            }
        }

        Ok(project)
    }
}

/// Round a time to 4 decimal places, matching the prototype's `round(t, 4)`.
///
/// Only the *emitted* value is rounded; the running clock stays unrounded so the
/// accumulator does not drift (the prototype keeps `t` in full precision).
fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Project an edit [`Project`] onto a parsed source [`Cast`], returning the
/// composed cast (normalized to a **v2** absolute-time header).
///
/// Reproduces `prototype/compose.py` step for step so golden parity holds:
/// per-segment inclusive windows over the source `o` events, inter-event delta
/// rebase + `idleCap` clamp + `1/speed` scale, a fixed [`BEAT`] before every
/// non-first non-empty segment, and a per-segment `holdEnd` zero-width freeze
/// tick. The composed header is the source header cloned whole (preserving
/// `timestamp`/`theme`/`env`), with `width`/`height` overridden from the project
/// `output` block and `version` forced to `2`.
///
/// This is pure event-stream math; the source `.cast` is never mutated.
#[must_use]
pub fn compose(source: &Cast, project: &Project) -> Cast {
    // Header projection: clone the whole source header (keeps timestamp/theme/env),
    // override geometry from the output block, and normalize to v2 absolute-time.
    let mut header = source.header.clone();
    if let Some(output) = &project.output {
        if let Some(w) = output.width {
            header.width = w;
        }
        if let Some(h) = output.height {
            header.height = h;
        }
    }
    header.version = 2;

    let idle_cap = project.idle_cap;
    let mut events: Vec<Event> = Vec::new();
    let mut clock = 0.0_f64;
    // Set once any segment emits a payload; gates the end-hold tick (prototype's
    // `last_out_payload is not None`). Since a non-empty window always emits at
    // least one payload, this is effectively true by the hold check — kept for
    // faithful parity with the prototype.
    let mut has_emitted = false;

    for (i, seg) in project.segments.iter().enumerate() {
        // Window selection reads the parser-normalized source times. The prototype
        // windows on raw `ev[0]`; the parser clamps v2 backwards times before we
        // see them, so results can only differ for a source with backwards
        // timestamps straddling a window edge (not present in the sample). Flagged
        // as an intentional, documented divergence.
        let window: Vec<&Event> = source
            .events
            .iter()
            .filter(|ev| {
                ev.code == EventCode::Output && ev.time >= seg.src_start && ev.time <= seg.src_end
            })
            .collect();

        // Empty windows are skipped BEFORE the beat is charged, exactly as the
        // prototype's `if not window: continue` precedes `if i > 0:`.
        if window.is_empty() {
            continue;
        }

        // Index-based beat: any segment with index `i > 0` and a non-empty window
        // gets one BEAT before its events. If segment 0 was empty, segment 1 still
        // has `i == 1 > 0`, so the beat IS charged before it — this is the
        // prototype's `if i > 0:`, not "the first non-empty segment".
        if i > 0 {
            clock += BEAT;
        }

        let mut prev = window[0].time;
        for ev in &window {
            let mut dt = ev.time - prev;
            prev = ev.time;
            if dt < 0.0 {
                dt = 0.0;
            }
            dt = dt.min(idle_cap) / seg.speed;
            clock += dt;
            events.push(Event {
                time: round4(clock),
                code: EventCode::Output,
                data: ev.data.clone(),
            });
            has_emitted = true;
        }

        // End-hold: keep the last screen on-frame `holdEnd` seconds via a
        // zero-width `[t, "o", ""]` tick that extends the dwell.
        if seg.hold_end > 0.0 && has_emitted {
            clock += seg.hold_end;
            events.push(Event {
                time: round4(clock),
                code: EventCode::Output,
                data: String::new(),
            });
        }
    }

    Cast { header, events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast::Cast;

    const EPS: f64 = 1e-6;

    fn src(body: &str) -> Cast {
        let text = format!("{{\"version\": 2, \"width\": 80, \"height\": 24}}\n{body}");
        Cast::parse(&text).expect("synthetic source parses")
    }

    #[test]
    fn parse_defaults_idle_cap_and_speed() {
        let p = Project::parse(r#"{"source":"a.cast","segments":[{"srcStart":0.0,"srcEnd":1.0}]}"#)
            .unwrap();
        assert!((p.idle_cap - 0.4).abs() < EPS);
        assert!((p.segments[0].speed - 1.0).abs() < EPS);
        assert!((p.segments[0].hold_end - 0.0).abs() < EPS);
    }

    #[test]
    fn parse_coerces_zero_speed_to_one() {
        let p = Project::parse(
            r#"{"source":"a.cast","segments":[{"srcStart":0.0,"srcEnd":1.0,"speed":0.0}]}"#,
        )
        .unwrap();
        assert!((p.segments[0].speed - 1.0).abs() < EPS);
    }

    #[test]
    fn parse_rejects_non_finite_and_negative() {
        // Non-finite / negative idleCap.
        assert!(matches!(
            Project::parse(r#"{"source":"a","idleCap":-0.1,"segments":[]}"#),
            Err(ProjectError::NegativeIdleCap(_))
        ));
        // Negative speed (prototype would keep it; we harden to a hard error).
        assert!(matches!(
            Project::parse(
                r#"{"source":"a","segments":[{"srcStart":0.0,"srcEnd":1.0,"speed":-2.0}]}"#
            ),
            Err(ProjectError::NegativeSpeed { segment: 0, .. })
        ));
        // Malformed JSON.
        assert!(matches!(
            Project::parse("not json"),
            Err(ProjectError::InvalidJson(_))
        ));
    }

    #[test]
    fn single_segment_has_no_leading_beat() {
        let source = src("[0.0, \"o\", \"a\"]\n[0.2, \"o\", \"b\"]\n");
        let project =
            Project::parse(r#"{"source":"s","segments":[{"srcStart":0.0,"srcEnd":1.0}]}"#).unwrap();
        let composed = compose(&source, &project);
        assert_eq!(composed.events.len(), 2);
        // No beat before segment 0: first event stays at 0.0.
        assert!((composed.events[0].time - 0.0).abs() < EPS);
        assert!((composed.events[1].time - 0.2).abs() < EPS);
    }

    #[test]
    fn idle_cap_clamps_long_gap() {
        let source = src("[0.0, \"o\", \"a\"]\n[3.0, \"o\", \"b\"]\n");
        let project = Project::parse(
            r#"{"source":"s","idleCap":0.4,"segments":[{"srcStart":0.0,"srcEnd":5.0}]}"#,
        )
        .unwrap();
        let composed = compose(&source, &project);
        // The 3.0s gap is clamped to idleCap 0.4.
        assert!((composed.events[1].time - 0.4).abs() < EPS);
    }

    #[test]
    fn speed_scales_gaps() {
        let source = src("[0.0, \"o\", \"a\"]\n[0.2, \"o\", \"b\"]\n");
        let project = Project::parse(
            r#"{"source":"s","segments":[{"srcStart":0.0,"srcEnd":1.0,"speed":2.0}]}"#,
        )
        .unwrap();
        let composed = compose(&source, &project);
        // 0.2s gap / speed 2.0 = 0.1s.
        assert!((composed.events[1].time - 0.1).abs() < EPS);
    }

    #[test]
    fn hold_end_emits_zero_width_tick() {
        let source = src("[0.0, \"o\", \"a\"]\n");
        let project = Project::parse(
            r#"{"source":"s","segments":[{"srcStart":0.0,"srcEnd":1.0,"holdEnd":1.5}]}"#,
        )
        .unwrap();
        let composed = compose(&source, &project);
        assert_eq!(composed.events.len(), 2);
        let tick = &composed.events[1];
        assert_eq!(tick.code, EventCode::Output);
        assert_eq!(tick.data, "");
        assert!((tick.time - 1.5).abs() < EPS);
    }

    #[test]
    fn leading_empty_segment_still_charges_beat() {
        // Segment 0's window is empty; segment 1 (index > 0) is non-empty. The
        // beat MUST be charged before segment 1 — index-based, not "first
        // non-empty". (PLAN_REVIEW item 1.)
        let source = src("[5.0, \"o\", \"e\"]\n");
        let project = Project::parse(
            r#"{"source":"s","segments":[
                {"srcStart":0.0,"srcEnd":1.0},
                {"srcStart":4.0,"srcEnd":6.0}]}"#,
        )
        .unwrap();
        let composed = compose(&source, &project);
        assert_eq!(composed.events.len(), 1);
        // Beat (0.5) charged before the first (and only) emitted event.
        assert!((composed.events[0].time - 0.5).abs() < EPS);
    }

    #[test]
    fn composed_header_is_v2_with_output_geometry() {
        let source = src("[0.0, \"o\", \"a\"]\n");
        let project = Project::parse(
            r#"{"source":"s","output":{"width":120,"height":40},
                "segments":[{"srcStart":0.0,"srcEnd":1.0}]}"#,
        )
        .unwrap();
        let composed = compose(&source, &project);
        assert_eq!(composed.header.version, 2);
        assert_eq!(composed.header.width, 120);
        assert_eq!(composed.header.height, 40);
    }

    #[test]
    fn non_output_events_are_excluded_from_windows() {
        let source = src("[0.0, \"o\", \"a\"]\n[0.1, \"r\", \"90x30\"]\n[0.2, \"o\", \"b\"]\n");
        let project =
            Project::parse(r#"{"source":"s","segments":[{"srcStart":0.0,"srcEnd":1.0}]}"#).unwrap();
        let composed = compose(&source, &project);
        // Only the two `o` events survive; the resize is dropped (prototype parity).
        assert_eq!(composed.events.len(), 2);
        assert!(composed.events.iter().all(|e| e.code == EventCode::Output));
    }
}
