// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `.tptreplay` format (spec §10.5, M8): a recording of what happened in an app run, as
//! JSON Lines.
//!
//! Line 1 is the header: `{"format":"tptreplay","version":1,...}`. Every later line is one
//! event: `{"seq":n,"t_ns":t,"kind":"...","data":{...}}`. `seq` counts from 0 and `t_ns` never
//! goes back.
//!
//! Two readers:
//! - [`Mode::Strict`] refuses anything it does not understand: unknown kinds, malformed lines,
//!   gaps in `seq`, or time going backwards.
//! - [`Mode::Lenient`] skips unknown kinds and malformed lines and counts them, so an older
//!   reader can still show a newer recording.
//!
//! [`divergence`] compares a recording with the events of a new run. It compares kind and
//! data, and ignores timing, which varies between runs.
//!
//! Not yet: zstd compression (the todo names it; the dependency is not approved) and asset
//! files. Determinism limits are in `docs/replay-format.md` (R8).

use std::fmt;

use serde_json::{json, Map, Value};

/// The only format version this crate writes and reads.
pub const VERSION: u64 = 1;

/// Event kinds this version defines.
pub const KINDS: [&str; 6] = [
    "ipc_request",
    "ipc_response",
    "ipc_event",
    "input",
    "window",
    "protocol_load",
];

/// The first line of a recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub app: String,
    pub platform: String,
}

/// One recorded event.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub t_ns: u64,
    pub kind: String,
    pub data: Value,
}

/// A whole recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub events: Vec<Event>,
    /// Lines skipped by a lenient read. Always 0 for a strict read.
    pub skipped: usize,
}

/// How strictly to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Strict,
    Lenient,
}

/// Why a recording could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadError {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ReadError {}

/// Builds a recording. Events must be added in order, with time never going backwards.
pub struct Recorder {
    header: Header,
    out: String,
    seq: u64,
    last_t: u64,
}

impl Recorder {
    pub fn new(header: Header) -> Self {
        let line = json!({
            "format": "tptreplay",
            "version": VERSION,
            "app": header.app,
            "platform": header.platform,
        });
        let mut out = line.to_string();
        out.push('\n');
        Recorder {
            header,
            out,
            seq: 0,
            last_t: 0,
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Records one event. `kind` must be one of [`KINDS`]; anything else is refused, so a
    /// recording never contains a kind the strict reader would reject.
    pub fn record(&mut self, t_ns: u64, kind: &str, data: Value) -> Result<(), String> {
        if !KINDS.contains(&kind) {
            return Err(format!("`{kind}` is not a known event kind"));
        }
        if t_ns < self.last_t {
            return Err(format!("time went back: {t_ns} after {}", self.last_t));
        }
        let line = json!({ "seq": self.seq, "t_ns": t_ns, "kind": kind, "data": data });
        self.out.push_str(&line.to_string());
        self.out.push('\n');
        self.seq += 1;
        self.last_t = t_ns;
        Ok(())
    }

    /// The finished recording text.
    pub fn finish(self) -> String {
        self.out
    }
}

/// Reads a recording.
pub fn read(text: &str, mode: Mode) -> Result<Replay, ReadError> {
    let mut lines = text.lines().enumerate();
    let (_, first) = lines.next().ok_or(ReadError {
        line: 1,
        message: "empty recording".into(),
    })?;
    let header = read_header(first)?;

    let mut events = Vec::new();
    let mut skipped = 0;
    let mut expect_seq = 0u64;
    let mut last_t = 0u64;
    for (index, line) in lines {
        let number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let fail = |message: String| ReadError {
            line: number,
            message,
        };
        match parse_event(line) {
            Ok(event) => {
                if !KINDS.contains(&event.kind.as_str()) {
                    if mode == Mode::Strict {
                        return Err(fail(format!("unknown event kind `{}`", event.kind)));
                    }
                    skipped += 1;
                    continue;
                }
                if event.seq != expect_seq {
                    // Strict refuses a gap. Lenient keeps the event and resynchronises on it.
                    if mode == Mode::Strict {
                        return Err(fail(format!(
                            "sequence number {} where {expect_seq} was expected",
                            event.seq
                        )));
                    }
                    expect_seq = event.seq;
                }
                if event.t_ns < last_t {
                    if mode == Mode::Strict {
                        return Err(fail("time goes backwards".into()));
                    }
                    skipped += 1;
                    continue;
                }
                expect_seq += 1;
                last_t = event.t_ns;
                events.push(event);
            }
            Err(message) => {
                if mode == Mode::Strict {
                    return Err(fail(message));
                }
                skipped += 1;
            }
        }
    }
    Ok(Replay {
        header,
        events,
        skipped,
    })
}

fn read_header(line: &str) -> Result<Header, ReadError> {
    let fail = |message: String| ReadError { line: 1, message };
    let value: Value =
        serde_json::from_str(line).map_err(|e| fail(format!("header is not JSON: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| fail("header is not an object".into()))?;
    if obj.get("format").and_then(Value::as_str) != Some("tptreplay") {
        return Err(fail("not a tptreplay recording".into()));
    }
    let version = obj.get("version").and_then(Value::as_u64);
    if version != Some(VERSION) {
        return Err(fail(format!(
            "version {version:?} is not supported; this reader understands {VERSION}"
        )));
    }
    Ok(Header {
        app: obj
            .get("app")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        platform: obj
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_event(line: &str) -> Result<Event, String> {
    let value: Value = serde_json::from_str(line).map_err(|e| format!("not JSON: {e}"))?;
    let obj: &Map<String, Value> = value.as_object().ok_or("event is not an object")?;
    let seq = obj
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or("missing `seq`")?;
    let t_ns = obj
        .get("t_ns")
        .and_then(Value::as_u64)
        .ok_or("missing `t_ns`")?;
    let kind = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("missing `kind`")?
        .to_string();
    let data = obj.get("data").cloned().ok_or("missing `data`")?;
    Ok(Event {
        seq,
        t_ns,
        kind,
        data,
    })
}

/// Where a new run first stopped matching the recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    pub index: usize,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
    pub reason: String,
}

/// Compares the recorded events with a new run's events, by kind and data. Returns the first
/// difference, or `None` when they match. A run that stops early or runs long also diverges.
pub fn divergence(recorded: &[Event], replayed: &[Event]) -> Option<Divergence> {
    for (index, (want, got)) in recorded.iter().zip(replayed).enumerate() {
        if want.kind != got.kind {
            return Some(Divergence {
                index,
                expected: Some(json!({ "kind": want.kind })),
                actual: Some(json!({ "kind": got.kind })),
                reason: format!("kind differs: `{}` then `{}`", want.kind, got.kind),
            });
        }
        if want.data != got.data {
            return Some(Divergence {
                index,
                expected: Some(want.data.clone()),
                actual: Some(got.data.clone()),
                reason: format!("data differs for a `{}` event", want.kind),
            });
        }
    }
    if recorded.len() > replayed.len() {
        let index = replayed.len();
        return Some(Divergence {
            index,
            expected: Some(json!({ "kind": recorded[index].kind })),
            actual: None,
            reason: "the run stopped before the recording did".into(),
        });
    }
    if replayed.len() > recorded.len() {
        let index = recorded.len();
        return Some(Divergence {
            index,
            expected: None,
            actual: Some(json!({ "kind": replayed[index].kind })),
            reason: "the run went on after the recording ended".into(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            app: "demo".into(),
            platform: "windows".into(),
        }
    }

    fn sample() -> String {
        let mut r = Recorder::new(header());
        r.record(0, "ipc_request", json!({ "id": 1, "cmd": "ping" }))
            .unwrap();
        r.record(
            5,
            "ipc_response",
            json!({ "id": 1, "ok": true, "data": "pong" }),
        )
        .unwrap();
        r.record(9, "input", json!({ "kind": "click", "x": 3, "y": 4 }))
            .unwrap();
        r.finish()
    }

    #[test]
    fn golden_recording_text() {
        assert_eq!(
            sample(),
            concat!(
                "{\"app\":\"demo\",\"format\":\"tptreplay\",\"platform\":\"windows\",\"version\":1}\n",
                "{\"data\":{\"cmd\":\"ping\",\"id\":1},\"kind\":\"ipc_request\",\"seq\":0,\"t_ns\":0}\n",
                "{\"data\":{\"data\":\"pong\",\"id\":1,\"ok\":true},\"kind\":\"ipc_response\",\"seq\":1,\"t_ns\":5}\n",
                "{\"data\":{\"kind\":\"click\",\"x\":3,\"y\":4},\"kind\":\"input\",\"seq\":2,\"t_ns\":9}\n",
            )
        );
    }

    #[test]
    fn a_recording_reads_back_exactly() {
        let replay = read(&sample(), Mode::Strict).unwrap();
        assert_eq!(replay.header, header());
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.events[2].t_ns, 9);
        assert_eq!(replay.skipped, 0);
    }

    #[test]
    fn the_recorder_refuses_unknown_kinds_and_backwards_time() {
        let mut r = Recorder::new(header());
        assert!(r.record(0, "mystery", json!({})).is_err());
        r.record(10, "window", json!({})).unwrap();
        assert!(r.record(9, "window", json!({})).is_err());
    }

    #[test]
    fn a_newer_version_is_refused() {
        let text = sample().replacen("\"version\":1", "\"version\":2", 1);
        assert!(read(&text, Mode::Strict).is_err());
        assert!(read(&text, Mode::Lenient).is_err());
    }

    #[test]
    fn not_a_recording_is_refused() {
        assert!(read("{\"hello\":1}\n", Mode::Strict).is_err());
        assert!(read("", Mode::Strict).is_err());
    }

    #[test]
    fn strict_refuses_unknown_kinds_and_lenient_skips_them() {
        let text = format!(
            "{}{}\n",
            sample(),
            "{\"seq\":3,\"t_ns\":10,\"kind\":\"from_the_future\",\"data\":{}}"
        );
        assert!(read(&text, Mode::Strict).is_err());
        let lenient = read(&text, Mode::Lenient).unwrap();
        assert_eq!(lenient.events.len(), 3);
        assert_eq!(lenient.skipped, 1);
    }

    #[test]
    fn strict_refuses_gaps_and_lenient_resynchronises() {
        let text = sample().replace("\"seq\":1", "\"seq\":5");
        let err = read(&text, Mode::Strict).unwrap_err();
        assert_eq!(err.line, 3);
        let lenient = read(&text, Mode::Lenient).unwrap();
        assert_eq!(lenient.skipped, 0);
        assert_eq!(lenient.events.len(), 3);
        assert_eq!(lenient.events[1].seq, 5);
    }

    #[test]
    fn malformed_lines_are_reported_with_their_number() {
        let text = format!("{}not json\n", sample());
        let err = read(&text, Mode::Strict).unwrap_err();
        assert_eq!(err.line, 5);
    }

    #[test]
    fn identical_runs_do_not_diverge_even_when_timing_differs() {
        let recorded = read(&sample(), Mode::Strict).unwrap().events;
        let mut replayed = recorded.clone();
        for e in &mut replayed {
            e.t_ns += 1_000;
        }
        assert_eq!(divergence(&recorded, &replayed), None);
    }

    #[test]
    fn the_first_changed_event_is_reported() {
        let recorded = read(&sample(), Mode::Strict).unwrap().events;
        let mut replayed = recorded.clone();
        replayed[1].data = json!({ "id": 1, "ok": false });
        let d = divergence(&recorded, &replayed).unwrap();
        assert_eq!(d.index, 1);
        assert_eq!(d.expected, Some(recorded[1].data.clone()));
        assert!(d.reason.contains("data differs"));
    }

    #[test]
    fn a_short_or_long_run_diverges_at_the_end() {
        let recorded = read(&sample(), Mode::Strict).unwrap().events;
        let short = divergence(&recorded, &recorded[..2]).unwrap();
        assert_eq!(short.index, 2);
        assert!(short.actual.is_none());
        let mut long = recorded.clone();
        long.push(recorded[0].clone());
        let extra = divergence(&recorded, &long).unwrap();
        assert_eq!(extra.index, 3);
        assert!(extra.expected.is_none());
    }
}
