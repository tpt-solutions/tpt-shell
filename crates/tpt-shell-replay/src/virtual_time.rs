// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Rust-side Clock and Rng, virtualised for record and replay (spec §10.5).
//!
//! A [`Session`] is either recording or replaying. In recording mode a [`VirtualClock`] reads
//! the real clock it wraps and logs each reading as a `clock` event; a [`VirtualRng`] draws
//! from the generator it wraps and logs each value as an `rng` event. In replay mode neither
//! touches the real source: each read returns the next recorded value, in order. A read that
//! does not match the recording (wrong kind, or none left) is noted as the session's
//! divergence, and the read returns the last value it had, so the run keeps going and the
//! player can report the first difference.
//!
//! The clock and rng events share one ordered stream, so the order in which the app reads
//! them must be the same in both runs. That is the determinism the format relies on.
//!
//! Support: Full for the Rust side. Partial: the page-side shim (`js/replay`) virtualises
//! `Date.now`, `performance.now`, `Math.random` and timers, and is not wired into the event
//! stream yet.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tpt_shell_core::{Clock, Rng};

use crate::{Header, Recorder, Replay};

enum Mode {
    /// Recording. `None` after the recording has been finished.
    Record(Option<Recorder>),
    /// Replaying the clock and rng events of a recording, in order.
    Replay(Playback),
}

struct Playback {
    draws: VecDeque<(String, Value)>,
    last_mono: u64,
    last_wall: u64,
    last_rng: u64,
    divergence: Option<String>,
}

struct Inner {
    mode: Mode,
    started: Instant,
    last_t: u64,
}

/// A recording or replay in progress. Clones share the same state. Support: Full.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<Inner>>,
}

impl Session {
    /// Starts recording. Support: Full.
    pub fn record(header: Header) -> Self {
        Session {
            inner: Arc::new(Mutex::new(Inner {
                mode: Mode::Record(Some(Recorder::new(header))),
                started: Instant::now(),
                last_t: 0,
            })),
        }
    }

    /// Starts replaying a recording. The clock and rng events are taken from it in order.
    /// Support: Full.
    pub fn replay(replay: &Replay) -> Self {
        let draws = replay
            .events
            .iter()
            .filter(|e| e.kind == "clock" || e.kind == "rng")
            .map(|e| (e.kind.clone(), e.data.clone()))
            .collect();
        Session {
            inner: Arc::new(Mutex::new(Inner {
                mode: Mode::Replay(Playback {
                    draws,
                    last_mono: 0,
                    last_wall: 0,
                    last_rng: 0,
                    divergence: None,
                }),
                started: Instant::now(),
                last_t: 0,
            })),
        }
    }

    /// Whether this session replays rather than records. Support: Full.
    pub fn is_replaying(&self) -> bool {
        matches!(self.lock().mode, Mode::Replay(_))
    }

    /// A clock for the app to use. In recording mode it reads `real`; in replay mode `real` is
    /// not read. Support: Full.
    pub fn clock(&self, real: Box<dyn Clock>) -> VirtualClock {
        VirtualClock {
            session: self.clone(),
            real: Some(real),
        }
    }

    /// A generator for the app to use. Support: Full.
    pub fn rng<R: Rng>(&self, real: R) -> VirtualRng<R> {
        VirtualRng {
            session: self.clone(),
            real: Some(real),
        }
    }

    /// Records an app event such as an IPC call, at `t_ns` since the session started. Refused
    /// while replaying. Support: Full.
    pub fn record_event(&self, kind: &str, data: Value) -> Result<(), String> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let t = inner.started.elapsed().as_nanos() as u64;
        let t = t.max(inner.last_t);
        match &mut inner.mode {
            Mode::Record(Some(rec)) => {
                rec.record(t, kind, data)?;
                inner.last_t = t;
                Ok(())
            }
            Mode::Record(None) => Err("the recording has already been finished".into()),
            Mode::Replay(_) => Err("events cannot be recorded while replaying".into()),
        }
    }

    /// The first point where a replayed read did not match the recording, if any. Support:
    /// Full.
    pub fn divergence(&self) -> Option<String> {
        match &self.lock().mode {
            Mode::Replay(p) => p.divergence.clone(),
            Mode::Record(_) => None,
        }
    }

    /// Finishes a recording and returns its text. Support: Full.
    pub fn finish(&self) -> Result<String, String> {
        let mut guard = self.lock();
        match &mut guard.mode {
            Mode::Record(rec) => rec
                .take()
                .map(Recorder::finish)
                .ok_or_else(|| "the recording has already been finished".into()),
            Mode::Replay(_) => Err("a replay has no text to finish".into()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic in another thread must not stop the session from being read back.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One clock read. `real` is the value from the wrapped clock when recording.
    fn clock_read(&self, op: &str, real: Option<u64>) -> u64 {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let t = (inner.started.elapsed().as_nanos() as u64).max(inner.last_t);
        match &mut inner.mode {
            Mode::Record(Some(rec)) => {
                let ns = real.unwrap_or(0);
                let _ = rec.record(t, "clock", json!({ "op": op, "ns": ns }));
                inner.last_t = t;
                ns
            }
            Mode::Record(None) => real.unwrap_or(0),
            Mode::Replay(p) => {
                let last = if op == "wall" {
                    p.last_wall
                } else {
                    p.last_mono
                };
                match take_draw(p, "clock") {
                    Some(data) => {
                        let got_op = data.get("op").and_then(Value::as_str);
                        let ns = data.get("ns").and_then(Value::as_u64);
                        match (got_op == Some(op), ns) {
                            (true, Some(ns)) => {
                                if op == "wall" {
                                    p.last_wall = ns;
                                } else {
                                    p.last_mono = ns;
                                }
                                ns
                            }
                            _ => {
                                note(p, format!("clock read `{op}` but the recording has {data}"));
                                last
                            }
                        }
                    }
                    None => {
                        note(p, format!("clock read `{op}` after the recording ended"));
                        last
                    }
                }
            }
        }
    }

    /// One generator draw. `real` is the value from the wrapped generator when recording.
    fn rng_draw(&self, real: Option<u64>) -> u64 {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let t = (inner.started.elapsed().as_nanos() as u64).max(inner.last_t);
        match &mut inner.mode {
            Mode::Record(Some(rec)) => {
                let value = real.unwrap_or(0);
                let _ = rec.record(t, "rng", json!({ "value": value }));
                inner.last_t = t;
                value
            }
            Mode::Record(None) => real.unwrap_or(0),
            Mode::Replay(p) => match take_draw(p, "rng") {
                Some(data) => match data.get("value").and_then(Value::as_u64) {
                    Some(value) => {
                        p.last_rng = value;
                        value
                    }
                    None => {
                        note(p, "rng draw has no recorded value".into());
                        p.last_rng
                    }
                },
                None => {
                    note(p, "rng draw after the recording ended".into());
                    p.last_rng
                }
            },
        }
    }
}

/// Takes the next clock or rng draw. If the next one is of the other kind, that is a
/// divergence: the read has happened in a different order from the recording.
fn take_draw(p: &mut Playback, kind: &str) -> Option<Value> {
    match p.draws.front() {
        Some((k, _)) if k == kind => p.draws.pop_front().map(|(_, data)| data),
        Some((k, _)) => {
            let found = k.clone();
            note(
                p,
                format!("the app read a `{kind}` but the recording has a `{found}` next"),
            );
            None
        }
        None => None,
    }
}

fn note(p: &mut Playback, message: String) {
    if p.divergence.is_none() {
        p.divergence = Some(message);
    }
}

/// The virtualised clock. Support: Full.
pub struct VirtualClock {
    session: Session,
    real: Option<Box<dyn Clock>>,
}

impl Clock for VirtualClock {
    fn monotonic(&self) -> Duration {
        let real = self.real.as_ref().map(|c| c.monotonic().as_nanos() as u64);
        Duration::from_nanos(self.session.clock_read("monotonic", real))
    }

    fn wall(&self) -> SystemTime {
        let real = self
            .real
            .as_ref()
            .and_then(|c| c.wall().duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64);
        UNIX_EPOCH + Duration::from_nanos(self.session.clock_read("wall", real))
    }
}

/// The virtualised generator. Support: Full.
pub struct VirtualRng<R: Rng> {
    session: Session,
    real: Option<R>,
}

impl<R: Rng> Rng for VirtualRng<R> {
    fn next_u64(&mut self) -> u64 {
        let real = self.real.as_mut().map(Rng::next_u64);
        self.session.rng_draw(real)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read, Mode};
    use tpt_shell_core::{FakeClock, SeededRng};

    fn header() -> Header {
        Header {
            app: "demo".into(),
            platform: "test".into(),
        }
    }

    /// A fake clock the test can keep moving after it has been handed to a session.
    struct Shared(Arc<FakeClock>);

    impl Clock for Shared {
        fn monotonic(&self) -> Duration {
            self.0.monotonic()
        }

        fn wall(&self) -> SystemTime {
            self.0.wall()
        }
    }

    /// Records a few reads from a fake clock and a seeded generator, and returns the text.
    fn record_run() -> (String, Vec<u64>, Vec<u64>) {
        let session = Session::record(header());
        let clock_source = Arc::new(FakeClock::new());
        clock_source.advance(Duration::from_millis(5));
        let clock = session.clock(Box::new(Shared(clock_source.clone())));
        let mut rng = session.rng(SeededRng::new(42));
        let mut times = Vec::new();
        let mut draws = Vec::new();
        for _ in 0..3 {
            times.push(clock.monotonic().as_nanos() as u64);
            draws.push(rng.next_u64());
            clock_source.advance(Duration::from_millis(1));
        }
        (session.finish().unwrap(), times, draws)
    }

    #[test]
    fn recorded_reads_come_back_in_order_on_replay() {
        let (text, times, draws) = record_run();
        let replay = read(&text, Mode::Strict).unwrap();
        let session = Session::replay(&replay);
        // The app gets a clock and a generator that ignore the real sources, and ask in the
        // same order. Nothing is read from the real clock: the wrapped one is left out.
        let clock = session.clock(Box::new(FakeClock::new()));
        let mut rng = session.rng(SeededRng::new(7));
        for i in 0..3 {
            assert_eq!(clock.monotonic().as_nanos() as u64, times[i]);
            assert_eq!(rng.next_u64(), draws[i]);
        }
        assert_eq!(session.divergence(), None);
    }

    #[test]
    fn a_read_in_a_different_order_is_a_divergence() {
        let (text, _, _) = record_run();
        let replay = read(&text, Mode::Strict).unwrap();
        let session = Session::replay(&replay);
        let clock = session.clock(Box::new(FakeClock::new()));
        let mut rng = session.rng(SeededRng::new(7));
        // The recording starts with a clock read; the app draws first.
        let _ = rng.next_u64();
        let msg = session.divergence().expect("divergence");
        assert!(msg.contains("`rng`"), "{msg}");
        let _ = clock.monotonic();
    }

    #[test]
    fn reading_past_the_end_is_a_divergence_not_a_panic() {
        let session = Session::record(header());
        let source = Arc::new(FakeClock::new());
        let clock = session.clock(Box::new(Shared(source.clone())));
        let _ = clock.monotonic();
        let text = session.finish().unwrap();
        let replay = read(&text, Mode::Strict).unwrap();
        let session = Session::replay(&replay);
        let clock = session.clock(Box::new(FakeClock::new()));
        assert_eq!(clock.monotonic(), Duration::ZERO);
        assert_eq!(session.divergence(), None);
        // The recording has no second read, so this one is past the end.
        let _ = clock.monotonic();
        assert!(session.divergence().unwrap().contains("ended"));
    }

    #[test]
    fn app_events_are_recorded_and_refused_on_replay() {
        let session = Session::record(header());
        session
            .record_event("ipc_request", json!({ "id": 1 }))
            .unwrap();
        let text = session.finish().unwrap();
        assert!(read(&text, Mode::Strict).is_ok());
        assert!(session.record_event("ipc_request", json!({})).is_err());
        let replay = read(&text, Mode::Strict).unwrap();
        let replaying = Session::replay(&replay);
        assert!(replaying.record_event("ipc_request", json!({})).is_err());
        assert!(replaying.finish().is_err());
    }

    #[test]
    fn a_finished_recording_cannot_be_finished_again() {
        let session = Session::record(header());
        assert!(session.finish().is_ok());
        assert!(session.finish().is_err());
    }

    #[test]
    fn the_wall_clock_is_virtualised_too() {
        let session = Session::record(header());
        let source = FakeClock::new();
        source.set_wall(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        let clock = session.clock(Box::new(source));
        let at = clock.wall().duration_since(UNIX_EPOCH).unwrap();
        assert_eq!(at, Duration::from_secs(1_700_000_000));
        let text = session.finish().unwrap();
        let replay = read(&text, Mode::Strict).unwrap();
        let replaying = Session::replay(&replay);
        let clock = replaying.clock(Box::new(FakeClock::new()));
        assert_eq!(
            clock.wall().duration_since(UNIX_EPOCH).unwrap(),
            Duration::from_secs(1_700_000_000)
        );
    }
}
