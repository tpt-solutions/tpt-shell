// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The data model for time-travel debugging (spec §10.5, devtools).
//!
//! A [`Timeline`] holds a recording's events and a cursor. The cursor is the number of events
//! already applied, so 0 is before the first event and `len()` is after the last. The devtools
//! scrubber moves the cursor with [`Timeline::step_forward`], [`Timeline::step_back`] and
//! [`Timeline::seek`], and reads the state at the cursor with [`Timeline::state`]. The scrubber
//! UI itself is the devtools agent's work; this module is the model it reads.
//!
//! DOM snapshots are not in the recording (R8: the DOM is not captured by the format). A
//! snapshot taken during a run can be attached at the event it was taken at with
//! [`Timeline::attach_dom`]; the timeline then shows the latest one before the cursor.
//!
//! Support: Full for the model. Partial: the state is rebuilt from the start on each call (it
//! is O(events)), which is fine for the recordings a developer scrubs; the UI is routed to the
//! CLI agent.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Divergence, Event};

/// The state of the run at the cursor.
///
/// Support: Full.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// IPC request ids that were sent and have no response yet, in order.
    pub in_flight: Vec<u64>,
    /// The last response for each IPC request id.
    pub responses: BTreeMap<u64, Value>,
    /// The last input event (click, key, scroll or focus).
    pub last_input: Option<Value>,
    /// The last window event (resize, theme or focus).
    pub last_window: Option<Value>,
    /// Clock reads made so far.
    pub clock_reads: usize,
    /// Random draws made so far.
    pub rng_draws: usize,
}

/// A recording's events with a cursor.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    events: Vec<Event>,
    cursor: usize,
    dom: Vec<(usize, Value)>,
}

impl Timeline {
    /// A timeline at the start of the events.
    ///
    /// Support: Full.
    pub fn new(events: Vec<Event>) -> Self {
        Timeline {
            events,
            cursor: 0,
            dom: Vec::new(),
        }
    }

    /// The number of events.
    ///
    /// Support: Partial. No test calls it.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether there are no events.
    ///
    /// Support: Partial. No test calls it.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The cursor: how many events are applied.
    ///
    /// Support: Full.
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// The event the cursor is on, the one that would be applied next.
    ///
    /// Support: Full.
    pub fn next_event(&self) -> Option<&Event> {
        self.events.get(self.cursor)
    }

    /// Applies one more event. Returns false at the end.
    ///
    /// Support: Full.
    pub fn step_forward(&mut self) -> bool {
        if self.cursor < self.events.len() {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    /// Undoes one event. Returns false at the start.
    ///
    /// Support: Full.
    pub fn step_back(&mut self) -> bool {
        if self.cursor > 0 {
            self.cursor -= 1;
            true
        } else {
            false
        }
    }

    /// Moves the cursor to `position`, which may be at most `len()`.
    ///
    /// Support: Full.
    pub fn seek(&mut self, position: usize) -> Result<(), String> {
        if position > self.events.len() {
            return Err(format!(
                "position {position} is past the end of {} events",
                self.events.len()
            ));
        }
        self.cursor = position;
        Ok(())
    }

    /// Moves the cursor to the event where a replay first diverged, so the state there can be
    /// inspected. Returns the position.
    ///
    /// Support: Full.
    pub fn jump_to_divergence(&mut self, divergence: &Divergence) -> usize {
        self.cursor = divergence.index.min(self.events.len());
        self.cursor
    }

    /// Attaches a DOM snapshot taken just before the event at `index`.
    ///
    /// Support: Full.
    pub fn attach_dom(&mut self, index: usize, snapshot: Value) {
        self.dom.retain(|(i, _)| *i != index);
        self.dom.push((index, snapshot));
        self.dom.sort_by_key(|(i, _)| *i);
    }

    /// The latest DOM snapshot taken at or before the cursor, if any.
    ///
    /// Support: Full.
    pub fn dom_at_cursor(&self) -> Option<&Value> {
        self.dom
            .iter()
            .rev()
            .find(|(i, _)| *i <= self.cursor)
            .map(|(_, v)| v)
    }

    /// The state of the run after the events before the cursor.
    ///
    /// Support: Full.
    pub fn state(&self) -> State {
        let mut state = State::default();
        for event in &self.events[..self.cursor] {
            match event.kind.as_str() {
                "ipc_request" => {
                    if let Some(id) = event.data.get("id").and_then(Value::as_u64) {
                        state.in_flight.push(id);
                    }
                }
                "ipc_response" => {
                    if let Some(id) = event.data.get("id").and_then(Value::as_u64) {
                        state.in_flight.retain(|x| *x != id);
                        state.responses.insert(id, event.data.clone());
                    }
                }
                "input" => state.last_input = Some(event.data.clone()),
                "window" => state.last_window = Some(event.data.clone()),
                "clock" => state.clock_reads += 1,
                "rng" => state.rng_draws += 1,
                _ => {}
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: u64, kind: &str, data: Value) -> Event {
        Event {
            seq,
            t_ns: seq,
            kind: kind.into(),
            data,
        }
    }

    fn sample() -> Timeline {
        Timeline::new(vec![
            ev(0, "ipc_request", json!({ "id": 1, "cmd": "load" })),
            ev(1, "input", json!({ "kind": "click", "x": 1 })),
            ev(2, "ipc_request", json!({ "id": 2, "cmd": "save" })),
            ev(3, "ipc_response", json!({ "id": 1, "ok": true })),
            ev(4, "window", json!({ "resize": [800, 600] })),
            ev(5, "clock", json!({ "op": "monotonic", "ns": 9 })),
        ])
    }

    #[test]
    fn the_cursor_steps_and_stops_at_both_ends() {
        let mut t = sample();
        assert_eq!(t.position(), 0);
        assert!(!t.step_back());
        for _ in 0..6 {
            assert!(t.step_forward());
        }
        assert!(!t.step_forward());
        assert_eq!(t.position(), 6);
        assert!(t.next_event().is_none());
        assert!(t.step_back());
        assert_eq!(t.position(), 5);
    }

    #[test]
    fn seek_refuses_positions_past_the_end() {
        let mut t = sample();
        assert!(t.seek(6).is_ok());
        assert!(t.seek(7).is_err());
        assert_eq!(t.position(), 6);
    }

    #[test]
    fn state_at_any_event_shows_pending_and_answered_requests() {
        let mut t = sample();
        t.seek(3).unwrap();
        let s = t.state();
        assert_eq!(s.in_flight, vec![1, 2]);
        t.seek(4).unwrap();
        let s = t.state();
        assert_eq!(s.in_flight, vec![2]);
        assert_eq!(s.responses.get(&1), Some(&json!({ "id": 1, "ok": true })));
        assert_eq!(s.last_input, Some(json!({ "kind": "click", "x": 1 })));
        assert_eq!(s.last_window, None);
    }

    #[test]
    fn counts_clock_and_rng_reads() {
        let mut t = sample();
        t.seek(6).unwrap();
        assert_eq!(t.state().clock_reads, 1);
        assert_eq!(t.state().rng_draws, 0);
    }

    #[test]
    fn the_nearest_earlier_dom_snapshot_is_shown() {
        let mut t = sample();
        t.attach_dom(2, json!({ "at": 2 }));
        t.attach_dom(4, json!({ "at": 4 }));
        t.seek(1).unwrap();
        assert_eq!(t.dom_at_cursor(), None);
        t.seek(3).unwrap();
        assert_eq!(t.dom_at_cursor(), Some(&json!({ "at": 2 })));
        t.seek(5).unwrap();
        assert_eq!(t.dom_at_cursor(), Some(&json!({ "at": 4 })));
    }

    #[test]
    fn a_divergence_is_reached_by_jumping() {
        let mut t = sample();
        let d = Divergence {
            index: 3,
            expected: None,
            actual: None,
            reason: "data differs".into(),
        };
        assert_eq!(t.jump_to_divergence(&d), 3);
        assert_eq!(t.position(), 3);
        let far = Divergence { index: 99, ..d };
        assert_eq!(t.jump_to_divergence(&far), 6);
    }
}
