//! Asciinema `.cast` parser — the first real `asciicut-core` public API.
//!
//! Turns an asciinema **v2 or v3** `.cast` document into one version-normalized
//! in-memory model: a [`Header`] (`width`, `height`, `theme`, `env`, `timestamp`,
//! `version`) plus an ordered stream of [`Event`]s carrying **absolute** second
//! timestamps. Downstream engines (T03 frame-at-T, T05 compose) consume this one
//! uniform shape and never branch on the recorded cast version.
//!
//! The whole surface is a **pure**, string-in / model-out function
//! ([`Cast::parse`]): it performs no `std::fs`, time, thread, or process I/O, so
//! it compiles byte-identically to `wasm32-unknown-unknown` and preserves the
//! native ↔ wasm parity contract (SPEC §7.1). Reading the file from disk is the
//! caller's job (the CLI/server in later tasks; the test harness here). The
//! source `.cast` is immutable (SPEC §4.1) — the parser only reads.
//!
//! ## The v2 ↔ v3 divergence, normalized once
//!
//! | Aspect | v2 | v3 | Normalized |
//! |---|---|---|---|
//! | Event time | **absolute** seconds | **interval** (delta) since prev | **absolute** seconds |
//! | Terminal size | top-level `width`/`height` | nested `term.cols`/`term.rows` | `header.width`/`header.height` |
//! | Theme | top-level `theme` | nested `term.theme` | `header.theme` (opaque) |
//!
//! Delta-time accumulation is the headline: for v2 each raw `t` is already
//! absolute (the accumulator is *set*); for v3 each raw `t` is the gap since the
//! previous event (the accumulator is *incremented*). Either way [`Event::time`]
//! is monotonic absolute seconds. A backwards time is clamped to `0` so the
//! accumulator never rewinds — mirroring the prototype's `if dt < 0: dt = 0`.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

/// A parsed asciinema `.cast`: a [`Header`] plus an ordered [`Event`] stream with
/// absolute timestamps, normalized across cast versions.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    /// The decoded, version-normalized header.
    pub header: Header,
    /// Events in recorded order; [`Event::time`] is absolute seconds.
    pub events: Vec<Event>,
}

/// The version-normalized cast header.
///
/// `width`/`height` come from top-level fields (v2) or `term.cols`/`term.rows`
/// (v3); `theme` from top-level `theme` (v2) or `term.theme` (v3) and is kept
/// **opaque** (the two versions' theme shapes differ — later stages interpret
/// it, the parser only preserves it). `version`, `timestamp`, and `env` are
/// always top-level in both versions.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The asciicast format version (`2` or `3`).
    pub version: u8,
    /// Terminal width in columns.
    pub width: u16,
    /// Terminal height in rows.
    pub height: u16,
    /// Unix start timestamp, if the recording carried one.
    pub timestamp: Option<u64>,
    /// The theme object, preserved verbatim if present (shape differs by version).
    pub theme: Option<serde_json::Value>,
    /// Captured environment variables, if present. `BTreeMap` for deterministic,
    /// parity-friendly ordering.
    pub env: Option<BTreeMap<String, String>>,
}

/// A single normalized event: absolute `time`, typed `code`, and raw `data`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Absolute seconds from the start of the recording (v3 deltas accumulated).
    pub time: f64,
    /// The typed event code.
    pub code: EventCode,
    /// The event payload — for output, the already-unescaped terminal bytes.
    pub data: String,
}

/// The typed asciicast event code.
///
/// Unknown codes (including `i` input) are preserved as [`EventCode::Other`] so
/// nothing is silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventCode {
    /// `o` — terminal output.
    Output,
    /// `r` — terminal resize (`data` is `"<cols>x<rows>"`).
    Resize,
    /// `m` — a marker (`data` is the marker label).
    Marker,
    /// Any other code, preserved verbatim (e.g. `i` input).
    Other(String),
}

impl EventCode {
    /// Map a raw asciicast code string to a typed [`EventCode`].
    fn from_raw(code: &str) -> Self {
        match code {
            "o" => EventCode::Output,
            "r" => EventCode::Resize,
            "m" => EventCode::Marker,
            other => EventCode::Other(other.to_owned()),
        }
    }

    /// Render a typed [`EventCode`] back to its raw asciicast code string —
    /// the inverse of [`EventCode::from_raw`], used by the serializer.
    fn as_raw(&self) -> &str {
        match self {
            EventCode::Output => "o",
            EventCode::Resize => "r",
            EventCode::Marker => "m",
            EventCode::Other(other) => other,
        }
    }
}

/// Everything that can go wrong while parsing a `.cast`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The input was empty or contained no header line.
    EmptyInput,
    /// The header line was not a valid JSON object (carries the decoder message).
    InvalidHeader(String),
    /// The `version` field was absent or not `2`/`3` (carries the offending value).
    UnsupportedVersion(String),
    /// Neither `width`/`height` (v2) nor `term.cols`/`term.rows` (v3) were present.
    MissingDimensions,
    /// An event line (1-based line number) was not a valid `[f64, string, string]`
    /// array (carries the line number and decoder message).
    MalformedEvent { line: usize, message: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::EmptyInput => write!(f, "empty input: no header line"),
            ParseError::InvalidHeader(msg) => write!(f, "invalid header JSON: {msg}"),
            ParseError::UnsupportedVersion(v) => {
                write!(f, "unsupported asciicast version: {v} (expected 2 or 3)")
            }
            ParseError::MissingDimensions => write!(
                f,
                "header is missing terminal dimensions (width/height or term.cols/term.rows)"
            ),
            ParseError::MalformedEvent { line, message } => {
                write!(f, "malformed event on line {line}: {message}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// The `term` sub-object of a v3 header.
#[derive(Debug, Deserialize)]
struct TermInfo {
    cols: Option<u16>,
    rows: Option<u16>,
    theme: Option<serde_json::Value>,
}

/// The raw, on-disk header shape accepting **both** v2 and v3 layouts.
#[derive(Debug, Deserialize)]
struct RawHeader {
    /// Present but decoded loosely so a v1/garbage version yields a clear
    /// unsupported-version error rather than a decode failure.
    version: Option<serde_json::Value>,
    width: Option<u16>,
    height: Option<u16>,
    theme: Option<serde_json::Value>,
    timestamp: Option<u64>,
    env: Option<BTreeMap<String, String>>,
    term: Option<TermInfo>,
}

impl Cast {
    /// Parse an asciinema **v2 or v3** `.cast` document into a normalized [`Cast`].
    ///
    /// Line 0 is the header JSON object; subsequent non-blank, non-comment lines
    /// are `[time, code, data]` event arrays. v3 interval times are accumulated
    /// into absolute seconds; v2 absolute times are used as-is. Backwards times
    /// are clamped to `0` so the running clock never rewinds.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] for empty input, a malformed header, an unsupported
    /// `version`, missing terminal dimensions, or a malformed event line (with its
    /// 1-based line number).
    ///
    /// # Examples
    ///
    /// ```
    /// let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n[0.5, \"o\", \"hi\"]\n";
    /// let cast = asciicut_core::Cast::parse(src).unwrap();
    /// assert_eq!(cast.header.version, 2);
    /// assert_eq!(cast.events.len(), 1);
    /// assert!((cast.events[0].time - 0.5).abs() < 1e-6);
    /// ```
    pub fn parse(input: &str) -> Result<Cast, ParseError> {
        let mut lines = input.lines();

        // Line 0 — the header. Skip any leading blank lines defensively.
        let header_line = loop {
            match lines.next() {
                Some(line) if line.trim().is_empty() => continue,
                Some(line) => break line,
                None => return Err(ParseError::EmptyInput),
            }
        };

        let raw: RawHeader = serde_json::from_str(header_line)
            .map_err(|e| ParseError::InvalidHeader(e.to_string()))?;

        // Advisory 3: validate the version BEFORE any field fallback so a v1 cast
        // gets a clear unsupported-version error instead of a dimensions error.
        let version = match raw.version.as_ref().and_then(serde_json::Value::as_u64) {
            Some(2) => 2u8,
            Some(3) => 3u8,
            _ => {
                let shown = raw
                    .version
                    .as_ref()
                    .map_or_else(|| "<missing>".to_owned(), ToString::to_string);
                return Err(ParseError::UnsupportedVersion(shown));
            }
        };

        // Dimensions: top-level (v2) else nested `term.*` (v3).
        let term = raw.term;
        let width = raw
            .width
            .or_else(|| term.as_ref().and_then(|t| t.cols))
            .ok_or(ParseError::MissingDimensions)?;
        let height = raw
            .height
            .or_else(|| term.as_ref().and_then(|t| t.rows))
            .ok_or(ParseError::MissingDimensions)?;

        // Theme: top-level (v2) else nested `term.theme` (v3).
        let theme = raw.theme.or_else(|| term.and_then(|t| t.theme));

        let header = Header {
            version,
            width,
            height,
            timestamp: raw.timestamp,
            theme,
            env: raw.env,
        };

        // Events. `clock` is the absolute-second accumulator. For v2 each raw `t`
        // is absolute (set the clock); for v3 each raw `t` is a delta (advance the
        // clock). Backwards motion is clamped so the clock never rewinds.
        let is_v3 = version == 3;
        let mut clock = 0.0_f64;
        let mut events = Vec::new();

        // `enumerate` starts at 0 for the header line; event lines are therefore
        // reported with their true 1-based file position.
        for (idx, line) in input.lines().enumerate().skip(1) {
            let trimmed = line.trim();
            // Skip blank lines and v3 `#`-prefixed comment lines (Advisory 1).
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (raw_time, code, data): (f64, String, String) = serde_json::from_str(trimmed)
                .map_err(|e| ParseError::MalformedEvent {
                    line: idx + 1,
                    message: e.to_string(),
                })?;

            if is_v3 {
                // v3: raw_time is a delta since the previous event. Clamp negatives.
                clock += raw_time.max(0.0);
            } else {
                // v2: raw_time is absolute. Never let the clock go backwards.
                clock = raw_time.max(clock);
            }

            events.push(Event {
                time: clock,
                code: EventCode::from_raw(&code),
                data,
            });
        }

        Ok(Cast { header, events })
    }

    /// Serialize this [`Cast`] back to a `.cast` document — the inverse of
    /// [`Cast::parse`], normalized to **v2 absolute-time**.
    ///
    /// Emits one JSON header object line followed by one `[time, code, data]`
    /// JSON array per event. The header `version` is always written as `2` (the
    /// event times this crate emits are absolute by construction, so v2 is the
    /// single canonical on-disk shape); `width`/`height` are always written, and
    /// `timestamp`/`theme`/`env` are written when present so a themed or
    /// env-carrying source round-trips. Like [`Cast::parse`] this is **pure** —
    /// no I/O — so it holds the native ↔ wasm parity contract (SPEC §7.1).
    ///
    /// Compose only ever emits finite times, so the underlying `serde_json`
    /// float encoding (which would render a non-finite `f64` as `null`) is never
    /// exercised with a non-finite value.
    ///
    /// # Examples
    ///
    /// ```
    /// let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n[0.5, \"o\", \"hi\"]\n";
    /// let cast = asciicut_core::Cast::parse(src).unwrap();
    /// let text = cast.to_cast_string();
    /// // Round-trips back to an equal model.
    /// assert_eq!(asciicut_core::Cast::parse(&text).unwrap(), cast);
    /// ```
    #[must_use]
    pub fn to_cast_string(&self) -> String {
        let mut header = serde_json::Map::new();
        // Always normalize to v2 absolute-time on serialize.
        header.insert("version".to_owned(), serde_json::json!(2));
        header.insert("width".to_owned(), serde_json::json!(self.header.width));
        header.insert("height".to_owned(), serde_json::json!(self.header.height));
        if let Some(ts) = self.header.timestamp {
            header.insert("timestamp".to_owned(), serde_json::json!(ts));
        }
        if let Some(theme) = &self.header.theme {
            header.insert("theme".to_owned(), theme.clone());
        }
        if let Some(env) = &self.header.env {
            header.insert("env".to_owned(), serde_json::json!(env));
        }

        let mut out = serde_json::Value::Object(header).to_string();
        out.push('\n');
        for ev in &self.events {
            let line = serde_json::json!([ev.time, ev.code.as_raw(), ev.data]);
            out.push_str(&line.to_string());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_absolute_times() {
        let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n\
                   [0.5, \"o\", \"a\"]\n\
                   [1.25, \"o\", \"b\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert_eq!(cast.header.version, 2);
        assert_eq!(cast.header.width, 80);
        assert_eq!(cast.header.height, 24);
        assert_eq!(cast.events.len(), 2);
        assert!((cast.events[0].time - 0.5).abs() < 1e-6);
        // v2 times are absolute, used as-is (not accumulated).
        assert!((cast.events[1].time - 1.25).abs() < 1e-6);
        assert_eq!(cast.events[0].code, EventCode::Output);
    }

    #[test]
    fn v3_deltas_accumulate_to_absolute() {
        let src = "{\"version\": 3, \"term\": {\"cols\": 100, \"rows\": 30}}\n\
                   [0.5, \"o\", \"a\"]\n\
                   [0.25, \"o\", \"b\"]\n\
                   [1.0, \"m\", \"chapter\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert_eq!(cast.header.version, 3);
        // Dimensions from term.cols / term.rows.
        assert_eq!(cast.header.width, 100);
        assert_eq!(cast.header.height, 30);
        // Deltas accumulate: 0.5, then 0.75, then 1.75.
        assert!((cast.events[0].time - 0.5).abs() < 1e-6);
        assert!((cast.events[1].time - 0.75).abs() < 1e-6);
        assert!((cast.events[2].time - 1.75).abs() < 1e-6);
        assert_eq!(cast.events[2].code, EventCode::Marker);
        assert_eq!(cast.events[2].data, "chapter");
    }

    #[test]
    fn extracts_term_theme_for_v3() {
        let src = "{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24, \
                   \"theme\": {\"fg\": \"#fff\", \"bg\": \"#000\"}}}\n";
        let cast = Cast::parse(src).unwrap();
        let theme = cast.header.theme.expect("theme should be preserved");
        assert_eq!(theme["fg"], "#fff");
    }

    #[test]
    fn maps_all_event_codes() {
        let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n\
                   [0.0, \"o\", \"out\"]\n\
                   [0.1, \"r\", \"90x30\"]\n\
                   [0.2, \"m\", \"mark\"]\n\
                   [0.3, \"i\", \"in\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert_eq!(cast.events[0].code, EventCode::Output);
        assert_eq!(cast.events[1].code, EventCode::Resize);
        assert_eq!(cast.events[1].data, "90x30");
        assert_eq!(cast.events[2].code, EventCode::Marker);
        // Unknown/`i` codes are preserved, not dropped.
        assert_eq!(cast.events[3].code, EventCode::Other("i".to_owned()));
    }

    #[test]
    fn reads_top_level_env_and_timestamp() {
        let src = "{\"version\": 2, \"width\": 80, \"height\": 24, \
                   \"timestamp\": 1700000000, \
                   \"env\": {\"SHELL\": \"/bin/zsh\", \"TERM\": \"xterm\"}}\n";
        let cast = Cast::parse(src).unwrap();
        assert_eq!(cast.header.timestamp, Some(1700000000));
        let env = cast.header.env.expect("env should be present");
        assert_eq!(env["SHELL"], "/bin/zsh");
    }

    #[test]
    fn v2_backwards_time_is_clamped() {
        let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n\
                   [1.0, \"o\", \"a\"]\n\
                   [0.5, \"o\", \"b\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert!((cast.events[0].time - 1.0).abs() < 1e-6);
        // The clock never rewinds: the backwards event clamps to the prior time.
        assert!((cast.events[1].time - 1.0).abs() < 1e-6);
    }

    #[test]
    fn v3_negative_delta_is_clamped() {
        let src = "{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24}}\n\
                   [1.0, \"o\", \"a\"]\n\
                   [-0.5, \"o\", \"b\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert!((cast.events[0].time - 1.0).abs() < 1e-6);
        // Negative delta contributes 0 — the clock holds at 1.0.
        assert!((cast.events[1].time - 1.0).abs() < 1e-6);
    }

    #[test]
    fn skips_blank_and_comment_lines() {
        let src = "{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24}}\n\
                   # a v3 comment line\n\
                   [0.5, \"o\", \"a\"]\n\
                   \n\
                   [0.5, \"o\", \"b\"]\n";
        let cast = Cast::parse(src).unwrap();
        assert_eq!(cast.events.len(), 2);
        assert!((cast.events[1].time - 1.0).abs() < 1e-6);
    }

    #[test]
    fn empty_input_errors() {
        assert_eq!(Cast::parse(""), Err(ParseError::EmptyInput));
        assert_eq!(Cast::parse("   \n  \n"), Err(ParseError::EmptyInput));
    }

    #[test]
    fn invalid_header_errors() {
        let err = Cast::parse("not json\n").unwrap_err();
        assert!(matches!(err, ParseError::InvalidHeader(_)));
    }

    #[test]
    fn unsupported_version_errors() {
        let err = Cast::parse("{\"version\": 1, \"width\": 80, \"height\": 24}\n").unwrap_err();
        assert_eq!(err, ParseError::UnsupportedVersion("1".to_owned()));
        // Missing version too.
        let err = Cast::parse("{\"width\": 80, \"height\": 24}\n").unwrap_err();
        assert_eq!(err, ParseError::UnsupportedVersion("<missing>".to_owned()));
    }

    #[test]
    fn missing_dimensions_errors() {
        // A valid version but no width/height and no term.* — must not silently
        // default (the 80-col fallback belongs to compose, not the parser).
        let err = Cast::parse("{\"version\": 2}\n").unwrap_err();
        assert_eq!(err, ParseError::MissingDimensions);
        let err = Cast::parse("{\"version\": 3, \"term\": {}}\n").unwrap_err();
        assert_eq!(err, ParseError::MissingDimensions);
    }

    #[test]
    fn malformed_event_reports_line_number() {
        let src = "{\"version\": 2, \"width\": 80, \"height\": 24}\n\
                   [0.0, \"o\", \"ok\"]\n\
                   [not, valid]\n";
        let err = Cast::parse(src).unwrap_err();
        match err {
            ParseError::MalformedEvent { line, .. } => assert_eq!(line, 3),
            other => panic!("expected MalformedEvent, got {other:?}"),
        }
    }

    #[test]
    fn serializer_round_trips_a_v2_cast() {
        let src = "{\"version\": 2, \"width\": 120, \"height\": 40, \
                   \"timestamp\": 1700000000, \
                   \"theme\": {\"fg\": \"#fff\", \"bg\": \"#000\"}, \
                   \"env\": {\"SHELL\": \"/bin/zsh\"}}\n\
                   [0.0, \"o\", \"a\"]\n\
                   [0.4, \"o\", \"b\"]\n\
                   [3.0, \"o\", \"\"]\n";
        let cast = Cast::parse(src).unwrap();
        let text = cast.to_cast_string();
        // parse -> serialize -> parse yields an equal model (f64 round-trips).
        let reparsed = Cast::parse(&text).unwrap();
        assert_eq!(reparsed, cast);
        // The re-emitted header is v2 and preserves theme/env/timestamp.
        assert_eq!(reparsed.header.version, 2);
        assert_eq!(reparsed.header.timestamp, Some(1700000000));
        assert_eq!(reparsed.header.theme.unwrap()["bg"], "#000");
        assert_eq!(reparsed.header.env.unwrap()["SHELL"], "/bin/zsh");
    }

    #[test]
    fn serializer_forces_v2_even_from_v3_source() {
        // A v3 source normalizes to a v2 absolute-time document on serialize.
        let src = "{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24}}\n\
                   [0.5, \"o\", \"a\"]\n\
                   [0.25, \"o\", \"b\"]\n";
        let cast = Cast::parse(src).unwrap();
        let text = cast.to_cast_string();
        assert!(text.lines().next().unwrap().contains("\"version\":2"));
        let reparsed = Cast::parse(&text).unwrap();
        // Absolute times survive; the model is now flagged v2.
        assert_eq!(reparsed.header.version, 2);
        assert!((reparsed.events[1].time - 0.75).abs() < 1e-6);
        assert_eq!(reparsed.events, cast.events);
    }

    #[test]
    fn display_messages_are_informative() {
        assert!(ParseError::EmptyInput.to_string().contains("empty input"));
        assert!(ParseError::MissingDimensions
            .to_string()
            .contains("dimensions"));
        assert!(ParseError::UnsupportedVersion("1".to_owned())
            .to_string()
            .contains("expected 2 or 3"));
    }
}
