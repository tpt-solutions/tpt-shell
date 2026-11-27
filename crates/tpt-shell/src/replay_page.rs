// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Replays a recording's page input into the app's first window (spec §10.5). The input events are
//! the clicks and key presses the page reported, each with the path of the element it was on. The
//! app runs a timer on the UI thread that finds each element in a fresh page snapshot and performs
//! the click or key on it, one step per tick, then quits and reports what it did.
//!
//! What is not replayed: text typed into an input. A recording holds only the length of the text,
//! not the text, so an input change is reported and skipped. A key from a password field is
//! recorded as `[redacted]` and is skipped too. A step whose element is not on the page is
//! reported, and the run goes on.
//!
//! Support: Full on Windows, with the `agent` feature (the snapshot and action scripts are the
//! agent's). Tested end to end in `tests/replay_page.rs`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tpt_shell_agent::page::{self, Action, Node};
use tpt_shell_platform_windows::backend::{eval_window_json, request_quit, WindowId};
use tpt_shell_replay::Replay;

/// How often the timer runs a step.
pub(crate) const PERIOD: Duration = Duration::from_millis(150);
/// How long the run waits for the page to show elements before it gives up.
const GIVE_UP: Duration = Duration::from_secs(20);
/// How long one page script may take.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

/// One input the run performs. The element is the last step of the recorded path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    Click {
        role: String,
        name: String,
    },
    Key {
        role: String,
        name: String,
        key: String,
    },
}

/// The lines the run reports, shared with the caller that waits for them.
pub(crate) type Report = Arc<Mutex<Vec<String>>>;

/// The steps to perform and the report to fill.
pub(crate) type Plan = (Vec<Step>, Report);

/// The steps a recording gives, and the lines for the input events that cannot be replayed.
pub(crate) fn plan(replay: &Replay) -> (Vec<Step>, Vec<String>) {
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    for event in replay.events.iter().filter(|e| e.kind == "input") {
        let data = &event.data;
        let last = data["path"].as_array().and_then(|p| p.last());
        let role = last
            .and_then(|s| s["role"].as_str())
            .unwrap_or("")
            .to_string();
        let name = last
            .and_then(|s| s["name"].as_str())
            .unwrap_or("")
            .to_string();
        match data["kind"].as_str() {
            Some("click") => steps.push(Step::Click { role, name }),
            Some("key") => {
                let key = data["key"].as_str().unwrap_or("").to_string();
                if key == "[redacted]" {
                    skipped.push(format!(
                        "key in {role} {name:?}: not replayed, it was typed into a password field"
                    ));
                } else {
                    steps.push(Step::Key { role, name, key });
                }
            }
            Some("input") => skipped.push(format!(
                "input into {role} {name:?}: not replayed, the recording holds the length of the text, not the text"
            )),
            other => skipped.push(format!("{other:?} input: not replayed")),
        }
    }
    (steps, skipped)
}

/// The page's elements, or `None` while the page has none to show.
fn snapshot(id: WindowId) -> Result<Option<Vec<Node>>, String> {
    let text = eval_window_json(id, &page::snapshot_script(), SCRIPT_TIMEOUT)
        .map_err(|e| e.to_string())?;
    match text.as_deref() {
        None | Some("null") => Ok(None),
        Some(json) => {
            let snapshot = page::parse_snapshot(json)?;
            Ok(if snapshot.tree.is_empty() {
                None
            } else {
                Some(snapshot.tree)
            })
        }
    }
}

/// Performs one action on the page and returns the page's answer.
fn act(id: WindowId, action: &Action) -> Result<serde_json::Value, String> {
    let text = eval_window_json(id, &page::action_script(action), SCRIPT_TIMEOUT)
        .map_err(|e| e.to_string())?
        .ok_or("the page gave no result for the action")?;
    page::parse_action_result(&text)
}

/// The first element with the recorded role, and the recorded name when one was recorded.
fn find<'a>(nodes: &'a [Node], role: &str, name: &str) -> Option<&'a Node> {
    nodes
        .iter()
        .find(|n| n.role == role && (name.is_empty() || n.name == name))
}

/// Performs one step and returns its report line.
pub(crate) fn perform(id: WindowId, nodes: &[Node], step: &Step) -> String {
    match step {
        Step::Click { role, name } => match find(nodes, role, name) {
            None => format!("click {role} {name:?}: no such element on the page"),
            Some(node) => match act(
                id,
                &Action::Click {
                    ref_id: node.ref_id,
                },
            ) {
                Ok(_) => format!("click {role} {name:?}: ok"),
                Err(e) => format!("click {role} {name:?}: {e}"),
            },
        },
        Step::Key { role, name, key } => {
            // A key goes to the element it was on when it is still there, and to the focused one
            // otherwise.
            let ref_id = find(nodes, role, name).map(|n| n.ref_id);
            match act(
                id,
                &Action::Key {
                    ref_id,
                    key: key.clone(),
                },
            ) {
                Ok(_) => format!("key {key} on {role} {name:?}: ok"),
                Err(e) => format!("key {key} on {role} {name:?}: {e}"),
            }
        }
    }
}

/// The timer's work for one tick: waits for the page, performs the next step, and at the end
/// reports the page title and quits. It gives up with a line in the report after [`GIVE_UP`].
/// `next` is the index of the next step; `steps.len()` means the steps are done and the title is
/// due, and one past that means the run has finished.
pub(crate) fn tick(
    id: WindowId,
    steps: &[Step],
    next: &mut usize,
    started: Instant,
    report: &Report,
) {
    if *next > steps.len() {
        return;
    }
    let nodes = match snapshot(id) {
        Ok(Some(nodes)) => nodes,
        Ok(None) | Err(_) => {
            if started.elapsed() > GIVE_UP {
                push(
                    report,
                    "the page showed no elements in time; the replay stops".into(),
                );
                request_quit();
                *next = steps.len() + 1;
            }
            return;
        }
    };
    if *next == steps.len() {
        // The title is read as the page has it after the last step, and it is the last line.
        let title = eval_window_json(id, "document.title", SCRIPT_TIMEOUT)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<String>(&json).ok())
            .unwrap_or_default();
        push(report, format!("title after the replay: {title}"));
        request_quit();
        *next = steps.len() + 1;
        return;
    }
    let line = perform(id, &nodes, &steps[*next]);
    *next += 1;
    push(report, line);
}

fn push(report: &Report, line: String) {
    report.lock().unwrap_or_else(|p| p.into_inner()).push(line);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tpt_shell_replay::{Header, Mode, Recorder};

    fn replay_with(events: &[(&str, serde_json::Value)]) -> Replay {
        let mut recorder = Recorder::new(Header {
            app: "test".into(),
            platform: "test".into(),
        });
        for (i, (kind, data)) in events.iter().enumerate() {
            recorder
                .record(i as u64, kind, data.clone())
                .expect("record");
        }
        tpt_shell_replay::read(&recorder.finish(), Mode::Lenient).expect("read")
    }

    #[test]
    fn clicks_and_keys_become_steps_and_the_rest_is_reported() {
        let save = json!([{ "index": 0, "role": "button", "name": "Save" }]);
        let replay = replay_with(&[
            (
                "input",
                json!({ "kind": "click", "path": save, "x": 1, "y": 2 }),
            ),
            (
                "input",
                json!({ "kind": "input", "path": save, "value_length": 4 }),
            ),
            (
                "input",
                json!({ "kind": "key", "path": [], "key": "Enter" }),
            ),
            (
                "input",
                json!({ "kind": "key", "path": save, "key": "[redacted]" }),
            ),
            ("ipc_request", json!({ "id": 1, "cmd": "read", "args": {} })),
        ]);
        let (steps, skipped) = plan(&replay);
        assert_eq!(
            steps,
            vec![
                Step::Click {
                    role: "button".into(),
                    name: "Save".into()
                },
                Step::Key {
                    role: String::new(),
                    name: String::new(),
                    key: "Enter".into()
                },
            ]
        );
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped[0].contains("length of the text"), "{skipped:?}");
        assert!(skipped[1].contains("password"), "{skipped:?}");
    }

    #[test]
    fn find_takes_the_first_element_with_the_role_and_name() {
        let node = |reference: u64, role: &str, name: &str| -> Node {
            serde_json::from_value(json!({
                "ref": reference,
                "role": role,
                "name": name,
                "bounds": { "x": 0, "y": 0, "w": 1, "h": 1 },
                "children": [],
            }))
            .expect("a node")
        };
        let nodes = vec![node(1, "button", "Cancel"), node(2, "button", "Save")];
        assert_eq!(find(&nodes, "button", "Save").map(|n| n.ref_id), Some(2));
        assert_eq!(find(&nodes, "button", "").map(|n| n.ref_id), Some(1));
        assert!(find(&nodes, "textbox", "Name").is_none());
    }
}
