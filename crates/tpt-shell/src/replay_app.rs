// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Replays a recording of an app's commands through the app's own router, with no window (spec
//! §10.5). The player in `tpt-shell-replay` runs any [`Target`]; this one runs the recorded
//! `ipc_request` events through the router the app built, and produces the `ipc_response` events
//! the run would have produced. A CI job can therefore check that the commands still answer as
//! they did, without a display.
//!
//! What is compared: the reply to each request, its data or its error code and message. What is
//! not: the time a call took. The replayed response carries the recorded duration, so timing
//! never makes a run diverge. Pages, window events and clock reads are not reproduced here; a
//! recording's other events are not compared.
//!
//! Each request runs as the first window of the app, with that window's grants. A consent grant
//! is refused, since there is nobody to answer the question.

use std::collections::HashMap;

use serde_json::{json, Value};
use tpt_shell_capabilities::{ConsentPrompt, Engine};
use tpt_shell_ipc::{Caller, Limits, Router};
use tpt_shell_replay::player::{self, Outcome, Target};
use tpt_shell_replay::{Event, Mode, Replay};

/// Refuses every consent question. A replay has nobody to ask.
struct Refuse;

impl ConsentPrompt for Refuse {
    fn ask(&mut self, _window: &str, _capability: &str) -> bool {
        false
    }
}

/// Runs each recorded request through the router, as the replay's window.
struct RouterTarget {
    router: Router,
    engine: Engine,
    caller: Caller,
    /// The duration each request took in the recording, by request id. Timing is not compared, so
    /// the replayed response takes it from here.
    durations: HashMap<u64, u64>,
}

impl Target for RouterTarget {
    fn apply(&mut self, event: &Event) -> Result<Vec<Event>, String> {
        if event.kind != "ipc_request" {
            return Ok(Vec::new());
        }
        let id = event
            .data
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("a recorded request has no id")?;
        let cmd = event
            .data
            .get("cmd")
            .and_then(Value::as_str)
            .ok_or("a recorded request has no command name")?;
        let args = event.data.get("args").cloned().unwrap_or(Value::Null);
        let text = json!({ "v": 1, "id": id, "cmd": cmd, "args": args }).to_string();
        let reply = self.router.handle_authorized(
            &self.caller,
            &mut self.engine,
            &mut Refuse,
            text.as_bytes(),
            &Limits::default(),
        );
        let reply: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
        let duration_ns = self.durations.get(&id).copied().unwrap_or(0);
        let data = if reply["ok"] == Value::Bool(true) {
            json!({ "id": id, "ok": true, "data": reply["data"], "duration_ns": duration_ns })
        } else {
            json!({
                "id": id,
                "ok": false,
                "code": reply["error"]["code"],
                "message": reply["error"]["message"],
                "duration_ns": duration_ns,
            })
        };
        Ok(vec![Event {
            seq: 0,
            t_ns: event.t_ns,
            kind: "ipc_response".into(),
            data,
        }])
    }

    fn reproduces(&self, kind: &str) -> bool {
        kind == "ipc_response"
    }
}

/// Plays `replay` through `router`, as `window` at `origin`, with the grants in `engine`. The
/// origin must be the one the app's grants were made for, which is the platform's app origin.
pub(crate) fn play(
    router: Router,
    engine: Engine,
    window: &str,
    origin: &str,
    replay: &Replay,
    mode: Mode,
) -> Result<Outcome, String> {
    let durations = replay
        .events
        .iter()
        .filter(|e| e.kind == "ipc_response")
        .filter_map(|e| {
            let id = e.data.get("id")?.as_u64()?;
            let duration = e.data.get("duration_ns")?.as_u64()?;
            Some((id, duration))
        })
        .collect();
    let mut target = RouterTarget {
        router,
        engine,
        caller: Caller {
            window: window.to_string(),
            origin: origin.to_string(),
        },
        durations,
    };
    player::play(replay, &mut target, mode).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_shell_replay::{Header, Recorder};

    /// The Windows app origin, which is what the grants in these tests are made for.
    const ORIGIN: &str = "https://tpt-app.localhost";

    fn recording(events: &[(&str, Value)]) -> Replay {
        let mut recorder = Recorder::new(Header {
            app: "test".into(),
            platform: "test".into(),
        });
        for (i, (kind, data)) in events.iter().enumerate() {
            recorder
                .record(i as u64 * 10, kind, data.clone())
                .expect("record");
        }
        tpt_shell_replay::read(&recorder.finish(), Mode::Strict).expect("read")
    }

    fn doubling_router() -> Router {
        let mut router = Router::new();
        router.register_guarded(
            "double",
            "math",
            |_| tpt_shell_ipc::Target::None,
            |args| {
                let n = args["n"].as_i64().unwrap_or(0);
                Ok(json!({ "n": n * 2 }))
            },
        );
        router
    }

    fn engine_with_math() -> Engine {
        let mut engine = Engine::new();
        engine
            .add_grant(tpt_shell_capabilities::Grant {
                capability: "math".into(),
                window: "main".into(),
                origin: ORIGIN.into(),
                scope: tpt_shell_capabilities::Scope::Unrestricted,
                consent: false,
            })
            .expect("grant");
        engine
    }

    #[test]
    fn a_recorded_run_replays_clean_even_though_its_timing_differs() {
        let replay = recording(&[
            (
                "ipc_request",
                json!({ "id": 1, "cmd": "double", "args": { "n": 4 } }),
            ),
            (
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": { "n": 8 }, "duration_ns": 999 }),
            ),
        ]);
        let outcome = play(
            doubling_router(),
            engine_with_math(),
            "main",
            ORIGIN,
            &replay,
            Mode::Strict,
        )
        .expect("strict play");
        assert_eq!(outcome.divergence, None);
    }

    #[test]
    fn a_changed_answer_diverges_at_the_response() {
        let replay = recording(&[
            (
                "ipc_request",
                json!({ "id": 1, "cmd": "double", "args": { "n": 4 } }),
            ),
            (
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": { "n": 9 }, "duration_ns": 5 }),
            ),
        ]);
        let lenient = play(
            doubling_router(),
            engine_with_math(),
            "main",
            ORIGIN,
            &replay,
            Mode::Lenient,
        )
        .expect("lenient play");
        let divergence = lenient.divergence.expect("the answer changed");
        assert_eq!(divergence.index, 1);
        assert!(play(
            doubling_router(),
            engine_with_math(),
            "main",
            ORIGIN,
            &replay,
            Mode::Strict,
        )
        .is_err());
    }

    #[test]
    fn a_command_the_window_may_not_call_is_recorded_as_its_refusal() {
        // The recording says the call succeeded, but the window holds no grant now: it diverges.
        let replay = recording(&[
            (
                "ipc_request",
                json!({ "id": 1, "cmd": "double", "args": { "n": 4 } }),
            ),
            (
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": { "n": 8 }, "duration_ns": 1 }),
            ),
        ]);
        let outcome = play(
            doubling_router(),
            Engine::new(),
            "main",
            ORIGIN,
            &replay,
            Mode::Lenient,
        )
        .expect("lenient play");
        assert!(
            outcome.divergence.is_some(),
            "the refusal differs from the recording"
        );
    }
}
