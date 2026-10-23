// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `.tptreplay` format (spec §10.5, M8): a recording of what happened in an app run, as
//! JSON Lines.
//!
//! Line 1 is the header: `{"format":"tptreplay","version":2,...}`. A line with an `asset` key
//! carries a file the run used (version 2 only). Every other line is one event:
//! `{"seq":n,"t_ns":t,"kind":"...","data":{...}}`. `seq` counts from 0 and `t_ns` never goes
//! back.
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
//! Compression: [`Recorder::finish_compressed`] writes the same text as a zstd frame, and
//! [`read_compressed`] reads it back. Plain text still reads with [`read`]. Assets carry a
//! SHA-256 that the reader checks. Version 1 recordings still read; they have no assets.
//!
//! Modules: [`capture`] has one typed helper per event kind for the record side; [`virtual_time`]
//! makes the Clock and Rng traits replayable; [`player`] runs a recording against a target;
//! [`timeline`] is the data model for time-travel; [`report`] builds the bug-report bundle with
//! redaction.
//!
//! Support: Full for the format, readers, recorder and divergence. Partial: the capture hooks
//! inside a running app and the hidden player window are routed to the facade and the
//! platform crates.

use std::fmt;

use serde_json::{json, Map, Value};

/// Support: Full.
pub mod capture;
/// Support: Full.
pub mod codec;
/// Support: Full.
pub mod player;
/// Support: Full.
pub mod report;
/// Support: Full.
pub mod timeline;
/// Support: Full.
pub mod virtual_time;

/// The version this crate writes.
///
/// Support: Full.
pub const VERSION: u64 = 2;
/// The versions this crate reads. Version 1 had no assets and no clock or rng events.
///
/// Support: Full.
pub const READABLE_VERSIONS: [u64; 2] = [1, 2];

/// Event kinds this version defines.
///
/// Support: Full.
pub const KINDS: [&str; 8] = [
    "ipc_request",
    "ipc_response",
    "ipc_event",
    "input",
    "window",
    "protocol_load",
    "clock",
    "rng",
];

/// The largest asset the reader accepts, in bytes.
///
/// Support: Full.
pub const MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;

/// The first line of a recording.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub app: String,
    pub platform: String,
}

/// One recorded event.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub t_ns: u64,
    pub kind: String,
    pub data: Value,
}

/// A file the run used, such as an image the page loaded. Stored inline as base64.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A whole recording.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub version: u64,
    pub header: Header,
    pub assets: Vec<Asset>,
    pub events: Vec<Event>,
    /// Lines skipped by a lenient read. Always 0 for a strict read.
    pub skipped: usize,
}

/// How strictly to read.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Strict,
    Lenient,
}

/// Why a recording could not be read.
///
/// Support: Full.
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

/// Whether a name is safe as an asset name: 1 to 100 ASCII letters, digits, `-`, `_` or `.`,
/// and not `.` or `..`. Names never carry a path, so an asset cannot be written outside its
/// directory when a recording is unpacked.
///
/// Support: Full.
pub fn is_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Builds a recording. Events must be added in order, with time never going backwards.
///
/// Support: Full.
pub struct Recorder {
    header: Header,
    out: String,
    seq: u64,
    last_t: u64,
}

impl Recorder {
    /// Starts a recording.
    ///
    /// Support: Full.
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

    /// The header this recording was started with.
    ///
    /// Support: Full.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Records one event. `kind` must be one of [`KINDS`]; anything else is refused, so a
    /// recording never contains a kind the strict reader would reject.
    ///
    /// Support: Full.
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

    /// Stores a file the run used. The name must pass [`is_asset_name`] and the bytes must be
    /// at most [`MAX_ASSET_BYTES`].
    ///
    /// Support: Full.
    pub fn add_asset(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !is_asset_name(name) {
            return Err(format!("`{name}` is not a valid asset name"));
        }
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(format!(
                "asset `{name}` is larger than {MAX_ASSET_BYTES} bytes"
            ));
        }
        let line = json!({
            "asset": {
                "name": name,
                "len": bytes.len(),
                "sha256": sha256_hex(bytes),
                "data": codec::encode(bytes)
            }
        });
        self.out.push_str(&line.to_string());
        self.out.push('\n');
        Ok(())
    }

    /// The finished recording text.
    ///
    /// Support: Full.
    pub fn finish(self) -> String {
        self.out
    }

    /// The finished recording, compressed with zstd. [`read_compressed`] reads it back; the plain
    /// text from [`Recorder::finish`] still reads with [`read`].
    ///
    /// Support: Full. The bundled zstd library is compiled at build time, so a build needs a C
    /// compiler; the Rust-only build paths do not change.
    pub fn finish_compressed(self) -> Vec<u8> {
        // Level 3 is zstd's default. Encoding into a Vec cannot fail for a valid level.
        zstd::encode_all(self.out.as_bytes(), 3).unwrap_or_default()
    }
}

/// The largest decompressed recording the reader accepts, in bytes.
///
/// Support: Full.
pub const MAX_RECORDING_BYTES: usize = 256 * 1024 * 1024;

/// The first four bytes of a zstd frame (little-endian magic 0xFD2FB528).
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Reads a zstd-compressed recording, as [`Recorder::finish_compressed`] writes it.
///
/// A truncated or corrupt stream is refused with an error, and so is output larger than
/// [`MAX_RECORDING_BYTES`]. The decompressed text is then read exactly as [`read`] reads it.
///
/// Support: Full.
pub fn read_compressed(bytes: &[u8], mode: Mode) -> Result<Replay, ReadError> {
    let fail = |message: String| ReadError { line: 1, message };
    if bytes.len() < ZSTD_MAGIC.len() || bytes[..4] != ZSTD_MAGIC {
        return Err(fail("not a zstd-compressed recording".into()));
    }
    let mut out = Vec::new();
    let decoder = zstd::stream::read::Decoder::new(bytes)
        .map_err(|e| fail(format!("cannot start zstd decoding: {e}")))?;
    // One extra byte lets the cap be detected without reading past it.
    let mut limited = std::io::Read::take(decoder, MAX_RECORDING_BYTES as u64 + 1);
    std::io::Read::read_to_end(&mut limited, &mut out)
        .map_err(|e| fail(format!("compressed recording is damaged or truncated: {e}")))?;
    if out.len() > MAX_RECORDING_BYTES {
        return Err(fail(format!(
            "decompressed recording is larger than {MAX_RECORDING_BYTES} bytes"
        )));
    }
    let text = String::from_utf8(out).map_err(|_| fail("recording is not UTF-8".into()))?;
    read(&text, mode)
}

/// The SHA-256 of some bytes, as 64 lowercase hex digits.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads a recording.
///
/// Support: Full.
pub fn read(text: &str, mode: Mode) -> Result<Replay, ReadError> {
    let mut lines = text.lines().enumerate();
    let (_, first) = lines.next().ok_or(ReadError {
        line: 1,
        message: "empty recording".into(),
    })?;
    let (header, version) = read_header(first)?;

    let mut assets = Vec::new();
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
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                if mode == Mode::Strict {
                    return Err(fail(format!("not JSON: {e}")));
                }
                skipped += 1;
                continue;
            }
        };
        if value.get("asset").is_some() {
            if version < 2 {
                return Err(fail("assets need format version 2".into()));
            }
            match parse_asset(&value) {
                Ok(asset) => assets.push(asset),
                Err(message) if mode == Mode::Strict => return Err(fail(message)),
                Err(_) => skipped += 1,
            }
            continue;
        }
        match parse_event(&value) {
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
        version,
        header,
        assets,
        events,
        skipped,
    })
}

fn read_header(line: &str) -> Result<(Header, u64), ReadError> {
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
    match version {
        Some(v) if READABLE_VERSIONS.contains(&v) => {}
        _ => {
            return Err(fail(format!(
            "version {version:?} is not supported; this reader understands {READABLE_VERSIONS:?}"
        )))
        }
    }
    let header = Header {
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
    };
    Ok((header, version.unwrap_or(0)))
}

fn parse_event(value: &Value) -> Result<Event, String> {
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

fn parse_asset(value: &Value) -> Result<Asset, String> {
    let asset = value
        .get("asset")
        .and_then(Value::as_object)
        .ok_or("`asset` is not an object")?;
    let name = asset
        .get("name")
        .and_then(Value::as_str)
        .ok_or("asset has no `name`")?;
    if !is_asset_name(name) {
        return Err(format!("asset name `{name}` is not valid"));
    }
    let len = asset
        .get("len")
        .and_then(Value::as_u64)
        .ok_or("asset has no `len`")?;
    if len as usize > MAX_ASSET_BYTES {
        return Err(format!(
            "asset `{name}` is larger than {MAX_ASSET_BYTES} bytes"
        ));
    }
    let data = asset
        .get("data")
        .and_then(Value::as_str)
        .ok_or("asset has no `data`")?;
    let bytes = codec::decode(data, MAX_ASSET_BYTES)?;
    if bytes.len() as u64 != len {
        return Err(format!(
            "asset `{name}` holds {} bytes, not {len}",
            bytes.len()
        ));
    }
    // Version 2 recordings written before the hash was added have no `sha256`, and still read.
    if let Some(expected) = asset.get("sha256") {
        let expected = expected
            .as_str()
            .ok_or_else(|| format!("asset `{name}` has a `sha256` that is not text"))?;
        if expected != sha256_hex(&bytes) {
            return Err(format!(
                "asset `{name}` does not match its sha256; the recording is damaged or was edited"
            ));
        }
    }
    Ok(Asset {
        name: name.to_string(),
        bytes,
    })
}

/// Where a new run first stopped matching the recording.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    pub index: usize,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
    pub reason: String,
}

/// Compares the recorded events with a new run's events, by kind and data. Returns the first
/// difference, or `None` when they match. A run that stops early or runs long also diverges.
/// Support: Full.
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
                "{\"app\":\"demo\",\"format\":\"tptreplay\",\"platform\":\"windows\",\"version\":2}\n",
                "{\"data\":{\"cmd\":\"ping\",\"id\":1},\"kind\":\"ipc_request\",\"seq\":0,\"t_ns\":0}\n",
                "{\"data\":{\"data\":\"pong\",\"id\":1,\"ok\":true},\"kind\":\"ipc_response\",\"seq\":1,\"t_ns\":5}\n",
                "{\"data\":{\"kind\":\"click\",\"x\":3,\"y\":4},\"kind\":\"input\",\"seq\":2,\"t_ns\":9}\n",
            )
        );
    }

    #[test]
    fn golden_asset_line() {
        let mut r = Recorder::new(header());
        r.add_asset("logo.png", b"abc").unwrap();
        assert_eq!(
            r.finish().lines().nth(1).unwrap(),
            "{\"asset\":{\"data\":\"YWJj\",\"len\":3,\"name\":\"logo.png\",\"sha256\":\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"}}"
        );
    }

    fn compressed_sample() -> Vec<u8> {
        let mut r = Recorder::new(header());
        r.record(5, "input", json!({"key": "a"})).unwrap();
        r.add_asset("logo.png", b"abc").unwrap();
        r.finish_compressed()
    }

    #[test]
    fn a_compressed_recording_round_trips() {
        let packed = compressed_sample();
        assert_eq!(packed[..4], ZSTD_MAGIC, "the output is a zstd frame");
        let replay = read_compressed(&packed, Mode::Strict).unwrap();
        assert_eq!(replay.header, header());
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.assets[0].bytes, b"abc");
    }

    #[test]
    fn a_truncated_compressed_recording_is_refused_not_panicked() {
        let packed = compressed_sample();
        for cut in [4, packed.len() / 2, packed.len() - 1] {
            assert!(
                read_compressed(&packed[..cut], Mode::Lenient).is_err(),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn plain_text_is_refused_by_the_compressed_reader_and_compressed_by_the_plain_reader() {
        assert!(read_compressed(sample().as_bytes(), Mode::Strict).is_err());
        let as_text = String::from_utf8_lossy(&compressed_sample()).into_owned();
        assert!(read(&as_text, Mode::Strict).is_err());
    }

    #[test]
    fn an_asset_whose_bytes_do_not_match_its_hash_is_refused() {
        let good = "{\"asset\":{\"data\":\"YWJj\",\"len\":3,\"name\":\"x.bin\",\"sha256\":\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"}}";
        let bad = good.replace("YWJj", "YWJk");
        let header = "{\"format\":\"tptreplay\",\"version\":2,\"app\":\"a\",\"platform\":\"p\"}\n";
        assert!(read(&format!("{header}{good}\n"), Mode::Strict).is_ok());
        let err = read(&format!("{header}{bad}\n"), Mode::Strict).unwrap_err();
        assert!(err.message.contains("sha256"), "{}", err.message);
        assert!(read(&format!("{header}{bad}\n"), Mode::Lenient)
            .unwrap()
            .assets
            .is_empty());
    }

    #[test]
    fn a_version_two_asset_without_a_hash_still_reads() {
        let header = "{\"format\":\"tptreplay\",\"version\":2,\"app\":\"a\",\"platform\":\"p\"}\n";
        let old = "{\"asset\":{\"data\":\"YWJj\",\"len\":3,\"name\":\"x.bin\"}}\n";
        assert_eq!(
            read(&format!("{header}{old}"), Mode::Strict)
                .unwrap()
                .assets
                .len(),
            1
        );
    }

    #[test]
    fn a_recording_reads_back_exactly() {
        let replay = read(&sample(), Mode::Strict).unwrap();
        assert_eq!(replay.header, header());
        assert_eq!(replay.version, 2);
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.events[2].t_ns, 9);
        assert_eq!(replay.skipped, 0);
    }

    #[test]
    fn assets_round_trip_with_their_bytes() {
        let mut r = Recorder::new(header());
        let png: Vec<u8> = (0..=255).collect();
        r.add_asset("shot.png", &png).unwrap();
        r.record(0, "window", json!({ "resize": [800, 600] }))
            .unwrap();
        let replay = read(&r.finish(), Mode::Strict).unwrap();
        assert_eq!(
            replay.assets,
            vec![Asset {
                name: "shot.png".into(),
                bytes: png
            }]
        );
        assert_eq!(replay.events.len(), 1);
    }

    #[test]
    fn asset_names_cannot_carry_a_path() {
        let mut r = Recorder::new(header());
        for bad in ["../x.png", "a/b.png", "a\\b.png", "..", "", "x y.png"] {
            assert!(r.add_asset(bad, b"x").is_err(), "{bad}");
        }
        let tampered = concat!(
            "{\"format\":\"tptreplay\",\"version\":2,\"app\":\"a\",\"platform\":\"p\"}\n",
            "{\"asset\":{\"data\":\"YWJj\",\"len\":3,\"name\":\"../evil\"}}\n",
        );
        assert!(read(tampered, Mode::Strict).is_err());
        assert!(read(tampered, Mode::Lenient).unwrap().assets.is_empty());
    }

    #[test]
    fn an_asset_whose_length_is_wrong_is_refused() {
        let text = concat!(
            "{\"format\":\"tptreplay\",\"version\":2,\"app\":\"a\",\"platform\":\"p\"}\n",
            "{\"asset\":{\"data\":\"YWJj\",\"len\":5,\"name\":\"x.bin\"}}\n",
        );
        assert!(read(text, Mode::Strict).is_err());
    }

    #[test]
    fn version_one_recordings_still_read_but_cannot_hold_assets() {
        let v1 = sample().replacen("\"version\":2", "\"version\":1", 1);
        assert_eq!(read(&v1, Mode::Strict).unwrap().version, 1);
        let with_asset = format!(
            "{}{{\"asset\":{{\"data\":\"YWJj\",\"len\":3,\"name\":\"x.bin\"}}}}\n",
            v1
        );
        assert!(read(&with_asset, Mode::Strict).is_err());
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
        let text = sample().replacen("\"version\":2", "\"version\":3", 1);
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
        assert_eq!(read(&text, Mode::Lenient).unwrap().skipped, 1);
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

    /// A small deterministic generator for the mutation test below.
    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn mutated_recordings_never_panic_the_reader() {
        let mut r = Recorder::new(header());
        r.add_asset("a.bin", &[1, 2, 3, 4, 5]).unwrap();
        r.record(0, "clock", json!({ "op": "monotonic", "ns": 7 }))
            .unwrap();
        r.record(4, "rng", json!({ "value": 99 })).unwrap();
        let good = r.finish().into_bytes();
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..5000 {
            let mut bytes = good.clone();
            for _ in 0..=(xorshift(&mut state) % 6) {
                let at = (xorshift(&mut state) as usize) % bytes.len();
                bytes[at] = xorshift(&mut state) as u8;
            }
            if xorshift(&mut state).is_multiple_of(5) {
                let cut = (xorshift(&mut state) as usize) % bytes.len();
                bytes.truncate(cut);
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let _ = read(&text, Mode::Strict);
            let _ = read(&text, Mode::Lenient);
        }
    }
}
