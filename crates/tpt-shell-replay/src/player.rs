// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Plays a recording against a target (spec §10.5).
//!
//! The target is whatever runs the app: in CI, a hidden window driven by the platform crates;
//! in tests, a fake. For each recorded event the player calls [`Target::apply`], which returns
//! the events the target produced in response (an IPC response, a window event, and so on).
//! The produced events are compared with the recording by [`crate::divergence`].
//!
//! - In [`Mode::Strict`] the first divergence is an error. This is what CI and the
//!   100-run determinism check use.
//! - In [`Mode::Lenient`] the run completes and the divergence is returned in the outcome,
//!   so a cross-OS replay can report the differences rather than stop.
//!
//! Support: Full for the player logic. Partial: the hidden window that hosts the target is
//! routed to the platform crates; this module does not create a window.

use serde_json::Value;

use crate::{divergence, Divergence, Event, Mode, Replay};

/// Something that runs the app for a replay.
pub trait Target {
    /// Performs one recorded event and returns the events the target produced. Support: Full.
    fn apply(&mut self, event: &Event) -> Result<Vec<Event>, String>;

    /// Whether the target reproduces events of this kind. The player compares only those with
    /// the recording. Inputs and requests are performed by the target, so they are not compared;
    /// responses, window events, protocol loads and clock or rng reads are what the run
    /// produces, and those must match. Support: Full.
    fn reproduces(&self, kind: &str) -> bool {
        matches!(
            kind,
            "ipc_response" | "ipc_event" | "window" | "protocol_load" | "clock" | "rng"
        )
    }
}

/// The result of a play that completed.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The events the target produced, in order.
    pub replayed: Vec<Event>,
    /// The first difference from the recording, if any. Always `None` in strict mode, where a
    /// difference is an error instead.
    pub divergence: Option<Divergence>,
}

/// Why a play stopped.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayError {
    /// The target failed on a recorded event.
    Target { index: usize, message: String },
    /// Strict mode found a difference.
    Diverged(Divergence),
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayError::Target { index, message } => {
                write!(f, "event {index}: the target failed: {message}")
            }
            PlayError::Diverged(d) => {
                write!(f, "replay diverged at event {}: {}", d.index, d.reason)
            }
        }
    }
}

impl std::error::Error for PlayError {}

/// Plays `replay` against `target`. Support: Full.
pub fn play(replay: &Replay, target: &mut dyn Target, mode: Mode) -> Result<Outcome, PlayError> {
    let mut replayed = Vec::new();
    for (index, event) in replay.events.iter().enumerate() {
        let produced = target
            .apply(event)
            .map_err(|message| PlayError::Target { index, message })?;
        replayed.extend(produced);
    }
    // Only the events the target reproduces are compared. The index of a difference is then
    // mapped back to its position in the whole recording, so reports point at the right line.
    let positions: Vec<usize> = (0..replay.events.len())
        .filter(|&i| target.reproduces(&replay.events[i].kind))
        .collect();
    let expected: Vec<Event> = positions
        .iter()
        .map(|&i| replay.events[i].clone())
        .collect();
    let found = divergence(&expected, &replayed).map(|mut d| {
        d.index = positions
            .get(d.index)
            .copied()
            .unwrap_or(replay.events.len());
        d
    });
    match (mode, found) {
        (Mode::Strict, Some(d)) => Err(PlayError::Diverged(d)),
        (_, divergence) => Ok(Outcome {
            replayed,
            divergence,
        }),
    }
}

/// A target that replays the recorded input events by echoing the recorded responses. It is
/// the reference used to check the player itself: it produces what the recording says it
/// should, so a play against it must not diverge. Support: Full.
pub struct Echo;

impl Target for Echo {
    fn apply(&mut self, event: &Event) -> Result<Vec<Event>, String> {
        Ok(match event.kind.as_str() {
            "ipc_request" => {
                let id = event.data.get("id").cloned().unwrap_or(Value::Null);
                vec![Event {
                    seq: 0,
                    t_ns: event.t_ns,
                    kind: "ipc_response".into(),
                    data: serde_json::json!({ "id": id, "ok": true, "data": event.data.get("cmd") }),
                }]
            }
            _ => Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read, Header, Recorder};
    use serde_json::json;

    /// A recording whose responses are exactly what `Echo` produces.
    fn recording() -> Replay {
        let mut r = Recorder::new(Header {
            app: "demo".into(),
            platform: "test".into(),
        });
        r.record(0, "ipc_request", json!({ "id": 1, "cmd": "ping" }))
            .unwrap();
        r.record(
            1,
            "ipc_response",
            json!({ "id": 1, "ok": true, "data": "ping" }),
        )
        .unwrap();
        r.record(2, "input", json!({ "kind": "click" })).unwrap();
        read(&r.finish(), Mode::Strict).unwrap()
    }

    #[test]
    fn a_faithful_target_plays_without_divergence_in_strict_mode() {
        let replay = recording();
        let outcome = play(&replay, &mut Echo, Mode::Strict).unwrap();
        assert_eq!(outcome.divergence, None);
        assert_eq!(outcome.replayed.len(), 1);
    }

    #[test]
    fn strict_mode_turns_a_difference_into_an_error() {
        let mut replay = recording();
        replay.events[1].data = json!({ "id": 1, "ok": false });
        let err = play(&replay, &mut Echo, Mode::Strict).unwrap_err();
        match err {
            PlayError::Diverged(d) => assert_eq!(d.index, 1),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn lenient_mode_reports_the_difference_and_finishes() {
        let mut replay = recording();
        replay.events[1].data = json!({ "id": 1, "ok": false });
        let outcome = play(&replay, &mut Echo, Mode::Lenient).unwrap();
        let d = outcome.divergence.expect("reported");
        assert_eq!(d.index, 1);
        assert!(d.reason.contains("data differs"));
    }

    #[test]
    fn a_silent_target_diverges_at_the_first_missing_response() {
        struct Silent;
        impl Target for Silent {
            fn apply(&mut self, _: &Event) -> Result<Vec<Event>, String> {
                Ok(Vec::new())
            }
        }
        let outcome = play(&recording(), &mut Silent, Mode::Lenient).unwrap();
        let d = outcome.divergence.expect("the run stopped early");
        // The first reproduced event is the response at recording index 1.
        assert_eq!(d.index, 1);
        assert!(d.reason.contains("stopped before"), "{}", d.reason);
    }

    #[test]
    fn a_failing_target_stops_with_its_event_index() {
        struct Broken;
        impl Target for Broken {
            fn apply(&mut self, _: &Event) -> Result<Vec<Event>, String> {
                Err("window closed".into())
            }
        }
        let err = play(&recording(), &mut Broken, Mode::Lenient).unwrap_err();
        assert_eq!(
            err,
            PlayError::Target {
                index: 0,
                message: "window closed".into()
            }
        );
    }
}
