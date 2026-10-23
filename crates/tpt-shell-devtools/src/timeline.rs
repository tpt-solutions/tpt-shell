// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The time-travel view of a recording (spec §10.5, devtools).
//!
//! [`Model`] wraps the replay crate's [`Timeline`], which owns the cursor, and adds what the
//! overlay shows: each event's kind and label, the IPC calls in flight or answered before the
//! cursor (keyed by command name), and the first divergence when a second run is compared with
//! the recording. The page never steps the recording itself: it asks the host, which moves the
//! model's cursor and sends back a new snapshot with [`Model::to_json`].
//!
//! Support: Full for the model. Partial: `to_json` lists every event, so a very long recording
//! makes a large snapshot, and the IPC view is rebuilt from the start on each call (it is O(events)).

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};
use tpt_shell_replay::timeline::Timeline;
use tpt_shell_replay::{divergence, Divergence, Event};

/// Where an IPC call stands at the cursor.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub enum CallState {
    /// Sent, and no reply before the cursor.
    Pending,
    /// Answered with data.
    Ok(Value),
    /// Answered with an error code and message.
    Failed { code: String, message: String },
}

/// The latest IPC call to a command, as it stands at the cursor.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct IpcCall {
    pub command: String,
    pub id: u64,
    pub state: CallState,
    /// How long the reply took, in the recording's clock, when it has been answered.
    pub duration_ns: Option<u64>,
}

/// A recording with a cursor, and what the overlay needs to show it.
///
/// Support: Full.
#[derive(Clone, Debug)]
pub struct Model {
    timeline: Timeline,
    events: Vec<Event>,
    divergence: Option<Divergence>,
}

impl Model {
    /// A model at the start of `events`, with no divergence.
    ///
    /// Support: Full.
    pub fn new(events: Vec<Event>) -> Model {
        Model {
            timeline: Timeline::new(events.clone()),
            events,
            divergence: None,
        }
    }

    /// A model for a recording compared with a second run. The first difference, if there is
    /// one, is kept so the cursor can jump to it.
    ///
    /// Support: Full.
    pub fn compared(recorded: Vec<Event>, replayed: &[Event]) -> Model {
        let found = divergence(&recorded, replayed);
        Model {
            timeline: Timeline::new(recorded.clone()),
            events: recorded,
            divergence: found,
        }
    }

    /// The number of events.
    ///
    /// Support: Full.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the recording has no events.
    ///
    /// Support: Full.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// How many events are applied: 0 is before the first, `len()` is after the last.
    ///
    /// Support: Full.
    pub fn position(&self) -> usize {
        self.timeline.position()
    }

    /// Applies one more event. Returns false at the end.
    ///
    /// Support: Full.
    pub fn step_forward(&mut self) -> bool {
        self.timeline.step_forward()
    }

    /// Undoes one event. Returns false at the start.
    ///
    /// Support: Full.
    pub fn step_back(&mut self) -> bool {
        self.timeline.step_back()
    }

    /// Moves the cursor to `position`, which may be at most `len()`.
    ///
    /// Support: Full.
    pub fn seek(&mut self, position: usize) -> Result<(), String> {
        self.timeline.seek(position)
    }

    /// Moves the cursor to just after the last event at or before `t_ns` on the recording's
    /// clock. A time before the first event puts the cursor at 0. Returns the position.
    ///
    /// Support: Full.
    pub fn jump_to_time(&mut self, t_ns: u64) -> usize {
        let position = self.events.iter().take_while(|e| e.t_ns <= t_ns).count();
        self.timeline
            .seek(position)
            .expect("a position counted from the events is within them");
        position
    }

    /// The index of the first event that differed from the second run, if there was one.
    ///
    /// Support: Full.
    pub fn divergence_index(&self) -> Option<usize> {
        self.divergence.as_ref().map(|d| d.index)
    }

    /// Moves the cursor to the first divergent event. Returns the position, or `None` when the
    /// runs matched and there is nothing to jump to.
    ///
    /// Support: Full.
    pub fn jump_to_divergence(&mut self) -> Option<usize> {
        let found = self.divergence.clone()?;
        Some(self.timeline.jump_to_divergence(&found))
    }

    /// The IPC calls made before the cursor, one per command name: the latest call to each.
    /// Ordered by command name.
    ///
    /// Support: Full.
    pub fn ipc_at_cursor(&self) -> Vec<IpcCall> {
        let mut requests: HashMap<u64, (String, u64)> = HashMap::new();
        let mut latest: BTreeMap<String, IpcCall> = BTreeMap::new();
        for event in &self.events[..self.position()] {
            match event.kind.as_str() {
                "ipc_request" => {
                    let (Some(id), Some(command)) = (
                        event.data.get("id").and_then(Value::as_u64),
                        event.data.get("cmd").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    requests.insert(id, (command.to_owned(), event.t_ns));
                    latest.insert(
                        command.to_owned(),
                        IpcCall {
                            command: command.to_owned(),
                            id,
                            state: CallState::Pending,
                            duration_ns: None,
                        },
                    );
                }
                "ipc_response" => {
                    let Some(id) = event.data.get("id").and_then(Value::as_u64) else {
                        continue;
                    };
                    let Some((command, _)) = requests.get(&id) else {
                        continue;
                    };
                    let state = if event.data.get("ok").and_then(Value::as_bool) == Some(true) {
                        CallState::Ok(event.data.get("data").cloned().unwrap_or(Value::Null))
                    } else {
                        CallState::Failed {
                            code: text(&event.data, "code"),
                            message: text(&event.data, "message"),
                        }
                    };
                    let duration_ns = event.data.get("duration_ns").and_then(Value::as_u64);
                    // Only the latest call to a command is shown, so a reply to an older call
                    // does not overwrite the state of a newer one.
                    if let Some(call) = latest.get_mut(command.as_str()) {
                        if call.id == id {
                            call.state = state;
                            call.duration_ns = duration_ns;
                        }
                    }
                }
                _ => {}
            }
        }
        latest.into_values().collect()
    }

    /// The model as JSON, for the overlay page. The host adds this under `timeline` in the
    /// snapshot it sends.
    ///
    /// Support: Full.
    pub fn to_json(&self) -> Value {
        let position = self.position();
        let calls: Vec<Value> = self
            .ipc_at_cursor()
            .into_iter()
            .map(|call| {
                let (state, data, code, message) = match call.state {
                    CallState::Pending => ("pending", Value::Null, Value::Null, Value::Null),
                    CallState::Ok(data) => ("ok", data, Value::Null, Value::Null),
                    CallState::Failed { code, message } => (
                        "failed",
                        Value::Null,
                        Value::String(code),
                        Value::String(message),
                    ),
                };
                json!({
                    "command": call.command,
                    "id": call.id,
                    "state": state,
                    "data": data,
                    "code": code,
                    "message": message,
                    "durationNs": call.duration_ns,
                })
            })
            .collect();
        json!({
            "len": self.len(),
            "position": position,
            "tNs": self.events[..position].last().map(|e| e.t_ns).unwrap_or(0),
            "events": self.events.iter().enumerate().map(|(index, e)| json!({
                "index": index,
                "seq": e.seq,
                "tNs": e.t_ns,
                "kind": e.kind,
                "label": label(e),
            })).collect::<Vec<_>>(),
            "ipc": calls,
            "divergence": self.divergence_index(),
        })
    }
}

/// A short label for an event: the command, the input kind, the window op, or the asset.
fn label(event: &Event) -> String {
    let field = match event.kind.as_str() {
        "ipc_request" => "cmd",
        "input" => "kind",
        "window" => "op",
        "protocol_load" => "asset",
        _ => return event.kind.clone(),
    };
    match event.data.get(field).and_then(Value::as_str) {
        Some(value) => format!("{} {value}", event.kind),
        None => event.kind.clone(),
    }
}

fn text(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, t_ns: u64, kind: &str, data: Value) -> Event {
        Event {
            seq,
            t_ns,
            kind: kind.into(),
            data,
        }
    }

    fn run() -> Vec<Event> {
        vec![
            ev(
                0,
                10,
                "ipc_request",
                json!({ "id": 1, "cmd": "load", "args": {} }),
            ),
            ev(
                1,
                20,
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": { "n": 1 }, "duration_ns": 10 }),
            ),
            ev(
                2,
                30,
                "ipc_request",
                json!({ "id": 2, "cmd": "save", "args": {} }),
            ),
            ev(
                3,
                40,
                "input",
                json!({ "kind": "click", "path": [], "x": 1, "y": 2 }),
            ),
            ev(
                4,
                50,
                "ipc_response",
                json!({ "id": 2, "ok": false, "code": "IPC_DENIED", "message": "no", "duration_ns": 20 }),
            ),
            ev(
                5,
                60,
                "ipc_request",
                json!({ "id": 3, "cmd": "load", "args": {} }),
            ),
        ]
    }

    #[test]
    fn the_cursor_steps_forward_and_back_and_stops_at_both_ends() {
        let mut m = Model::new(run());
        assert_eq!(m.position(), 0);
        assert!(!m.step_back());
        for _ in 0..6 {
            assert!(m.step_forward());
        }
        assert!(!m.step_forward());
        assert_eq!(m.position(), 6);
        assert!(m.step_back());
        assert_eq!(m.position(), 5);
    }

    #[test]
    fn a_time_puts_the_cursor_after_the_last_event_at_or_before_it() {
        let mut m = Model::new(run());
        assert_eq!(m.jump_to_time(5), 0);
        assert_eq!(m.jump_to_time(25), 2);
        assert_eq!(m.jump_to_time(50), 5);
        assert_eq!(m.jump_to_time(u64::MAX), 6);
        assert_eq!(m.position(), 6);
    }

    #[test]
    fn ipc_state_shows_pending_answered_and_failed_calls_at_the_cursor() {
        let mut m = Model::new(run());
        m.seek(3).unwrap();
        let calls = m.ipc_at_cursor();
        assert_eq!(calls.len(), 2);
        let load = calls.iter().find(|c| c.command == "load").unwrap();
        assert_eq!(load.state, CallState::Ok(json!({ "n": 1 })));
        assert_eq!(load.duration_ns, Some(10));
        let save = calls.iter().find(|c| c.command == "save").unwrap();
        assert_eq!(save.state, CallState::Pending);

        m.seek(5).unwrap();
        let save = m
            .ipc_at_cursor()
            .into_iter()
            .find(|c| c.command == "save")
            .unwrap();
        assert_eq!(
            save.state,
            CallState::Failed {
                code: "IPC_DENIED".into(),
                message: "no".into()
            }
        );
    }

    #[test]
    fn only_the_latest_call_to_a_command_is_shown() {
        let mut m = Model::new(run());
        m.seek(6).unwrap();
        let load = m
            .ipc_at_cursor()
            .into_iter()
            .find(|c| c.command == "load")
            .unwrap();
        // The second `load` (id 3) has no reply yet, so it is pending, not the first call's data.
        assert_eq!(load.id, 3);
        assert_eq!(load.state, CallState::Pending);
    }

    #[test]
    fn a_reply_to_an_older_call_does_not_overwrite_a_newer_one() {
        let events = vec![
            ev(0, 1, "ipc_request", json!({ "id": 1, "cmd": "load" })),
            ev(1, 2, "ipc_request", json!({ "id": 2, "cmd": "load" })),
            ev(
                2,
                3,
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": "old" }),
            ),
        ];
        let mut m = Model::new(events);
        m.seek(3).unwrap();
        let load = m.ipc_at_cursor().remove(0);
        assert_eq!(load.id, 2);
        assert_eq!(load.state, CallState::Pending);
    }

    #[test]
    fn the_cursor_jumps_to_the_first_divergent_event() {
        let recorded = run();
        let mut replayed = run();
        replayed[3].data = json!({ "kind": "click", "path": [], "x": 9, "y": 2 });
        let mut m = Model::compared(recorded, &replayed);
        assert_eq!(m.divergence_index(), Some(3));
        assert_eq!(m.jump_to_divergence(), Some(3));
        assert_eq!(m.position(), 3);
    }

    #[test]
    fn matching_runs_have_no_divergence_to_jump_to() {
        let mut m = Model::compared(run(), &run());
        assert_eq!(m.divergence_index(), None);
        assert_eq!(m.jump_to_divergence(), None);
        assert_eq!(m.position(), 0);
    }

    #[test]
    fn an_empty_recording_is_at_the_start_and_has_nothing_to_show() {
        let mut m = Model::new(Vec::new());
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert!(!m.step_forward());
        assert!(!m.step_back());
        assert_eq!(m.jump_to_time(100), 0);
        assert!(m.ipc_at_cursor().is_empty());
        let json = m.to_json();
        assert_eq!(json["events"], json!([]));
        assert_eq!(json["ipc"], json!([]));
        assert_eq!(json["divergence"], Value::Null);
        assert_eq!(json["tNs"], json!(0));
    }

    #[test]
    fn the_json_names_each_event_and_the_ipc_state_the_page_reads() {
        let mut m = Model::new(run());
        m.seek(4).unwrap();
        let json = m.to_json();
        assert_eq!(json["len"], json!(6));
        assert_eq!(json["position"], json!(4));
        assert_eq!(json["tNs"], json!(40));
        assert_eq!(json["events"][3]["label"], json!("input click"));
        assert_eq!(json["events"][0]["label"], json!("ipc_request load"));
        let ipc = json["ipc"].as_array().unwrap();
        let save = ipc.iter().find(|c| c["command"] == "save").unwrap();
        assert_eq!(save["state"], json!("pending"));
        let load = ipc.iter().find(|c| c["command"] == "load").unwrap();
        assert_eq!(load["state"], json!("ok"));
        assert_eq!(load["data"], json!({ "n": 1 }));
    }
}
