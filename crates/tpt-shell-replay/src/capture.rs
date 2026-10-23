// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Typed capture helpers for the record side of a run. Each helper builds the `data` of one event
//! kind and records it with [`Recorder::record`], so the shape of each kind is written in one place.
//! The in-page half is `js/replay/shim.js`, which writes the same shapes for DOM events and
//! protocol loads.
//!
//! Shapes:
//! - `ipc_request`: `{"id","cmd","args"}`
//! - `ipc_response`: `{"id","ok":true,"data","duration_ns"}`, or `{"id","ok":false,"code",
//!   "message","duration_ns"}`
//! - `input`: `{"kind":"click","path","x","y"}`, `{"kind":"key","path","key"}`, or
//!   `{"kind":"input","path","value_length"}`. An input's value is never recorded, only its length.
//! - `window`: `{"op":"resize","window","width","height"}`, `{"op":"focus","window"}`, or
//!   `{"op":"close","window"}`
//! - `protocol_load`: `{"asset","status"}`. The response body is not recorded.
//!
//! Support: Full for the shapes and the helpers. Partial: the facade does not call these yet, so a
//! running app records nothing through them.

use serde_json::{json, Value};

use crate::{is_asset_name, Recorder};

/// The most characters of an accessible name a path step keeps. Longer names are cut, so a
/// recording stays small and page text is not copied into it.
///
/// Support: Full.
pub const MAX_NAME_CHARS: usize = 80;

/// One step of an element path: the element's position among its siblings, its role and its
/// accessible name. A path lists the steps from the root down to the element. It uses no CSS
/// selector or id, which break when the page changes.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathStep {
    pub index: u32,
    pub role: String,
    pub name: String,
}

/// An input event on the page.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A pointer click at a position, with the element it landed on.
    Click { path: Vec<PathStep>, x: i64, y: i64 },
    /// A key press. `key` is the key's name (`Enter`, `a`); a key typed into a password field is
    /// recorded as `[redacted]` by the caller.
    Key { path: Vec<PathStep>, key: String },
    /// The value of an input changed. Only the length is recorded.
    Input {
        path: Vec<PathStep>,
        value_length: u64,
    },
}

/// A window event.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Resize {
        window: String,
        width: u32,
        height: u32,
    },
    Focus {
        window: String,
    },
    Close {
        window: String,
    },
}

/// Why an IPC call failed, as the router reports it.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpcFailure {
    pub code: String,
    pub message: String,
}

/// The most characters of a name a path step keeps.
fn truncate(name: &str) -> String {
    name.chars().take(MAX_NAME_CHARS).collect()
}

fn path_value(path: &[PathStep]) -> Value {
    Value::Array(
        path.iter()
            .map(|step| {
                json!({
                    "index": step.index,
                    "role": step.role,
                    "name": truncate(&step.name),
                })
            })
            .collect(),
    )
}

/// Records an IPC request as the router received it.
///
/// Support: Full.
pub fn ipc_request(
    recorder: &mut Recorder,
    t_ns: u64,
    id: u64,
    cmd: &str,
    args: &Value,
) -> Result<(), String> {
    recorder.record(
        t_ns,
        "ipc_request",
        json!({ "id": id, "cmd": cmd, "args": args }),
    )
}

/// Records the reply to an IPC request, with how long it took in the recording's clock.
///
/// Support: Full.
pub fn ipc_response(
    recorder: &mut Recorder,
    t_ns: u64,
    id: u64,
    duration_ns: u64,
    outcome: Result<&Value, &IpcFailure>,
) -> Result<(), String> {
    let data = match outcome {
        Ok(data) => json!({ "id": id, "ok": true, "data": data, "duration_ns": duration_ns }),
        Err(failure) => json!({
            "id": id,
            "ok": false,
            "code": failure.code,
            "message": failure.message,
            "duration_ns": duration_ns,
        }),
    };
    recorder.record(t_ns, "ipc_response", data)
}

/// Records an input event on the page.
///
/// Support: Full.
pub fn input(recorder: &mut Recorder, t_ns: u64, event: &Input) -> Result<(), String> {
    let data = match event {
        Input::Click { path, x, y } => json!({
            "kind": "click",
            "path": path_value(path),
            "x": x,
            "y": y,
        }),
        Input::Key { path, key } => json!({
            "kind": "key",
            "path": path_value(path),
            "key": key,
        }),
        Input::Input { path, value_length } => json!({
            "kind": "input",
            "path": path_value(path),
            "value_length": value_length,
        }),
    };
    recorder.record(t_ns, "input", data)
}

/// Records a window event. Names are recorded as given; the window name is not a path.
///
/// Support: Full.
pub fn window(recorder: &mut Recorder, t_ns: u64, event: &WindowEvent) -> Result<(), String> {
    let data = match event {
        WindowEvent::Resize {
            window,
            width,
            height,
        } => json!({ "op": "resize", "window": window, "width": width, "height": height }),
        WindowEvent::Focus { window } => json!({ "op": "focus", "window": window }),
        WindowEvent::Close { window } => json!({ "op": "close", "window": window }),
    };
    recorder.record(t_ns, "window", data)
}

/// Records that the page loaded an app asset, with its HTTP status. The body is not recorded.
/// The asset name must pass [`is_asset_name`], the same rule the reader applies to assets.
///
/// Support: Full.
pub fn protocol_load(
    recorder: &mut Recorder,
    t_ns: u64,
    asset: &str,
    status: u16,
) -> Result<(), String> {
    if !is_asset_name(asset) {
        return Err(format!("`{asset}` is not a valid asset name"));
    }
    if !(100..=599).contains(&status) {
        return Err(format!("status {status} is not an HTTP status code"));
    }
    recorder.record(
        t_ns,
        "protocol_load",
        json!({ "asset": asset, "status": status }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read, Header, Mode};

    fn header() -> Header {
        Header {
            app: "till".into(),
            platform: "windows".into(),
        }
    }

    fn step(index: u32, role: &str, name: &str) -> PathStep {
        PathStep {
            index,
            role: role.into(),
            name: name.into(),
        }
    }

    // Records one of every kind and returns the recording text.
    fn every_kind() -> String {
        let mut r = Recorder::new(header());
        let path = vec![step(0, "main", ""), step(1, "button", "Cancel")];
        ipc_request(&mut r, 1, 7, "sale.add", &json!({ "sku": "A1" })).unwrap();
        ipc_response(&mut r, 2, 7, 1_000, Ok(&json!({ "total": 3 }))).unwrap();
        input(
            &mut r,
            3,
            &Input::Click {
                path: path.clone(),
                x: 10,
                y: 20,
            },
        )
        .unwrap();
        input(
            &mut r,
            4,
            &Input::Key {
                path: path.clone(),
                key: "Enter".into(),
            },
        )
        .unwrap();
        input(
            &mut r,
            5,
            &Input::Input {
                path,
                value_length: 9,
            },
        )
        .unwrap();
        window(
            &mut r,
            6,
            &WindowEvent::Resize {
                window: "main".into(),
                width: 800,
                height: 600,
            },
        )
        .unwrap();
        window(
            &mut r,
            7,
            &WindowEvent::Focus {
                window: "main".into(),
            },
        )
        .unwrap();
        window(
            &mut r,
            8,
            &WindowEvent::Close {
                window: "main".into(),
            },
        )
        .unwrap();
        protocol_load(&mut r, 9, "index.html", 200).unwrap();
        r.finish()
    }

    #[test]
    fn every_kind_round_trips_through_the_strict_reader() {
        let replay = read(&every_kind(), Mode::Strict).unwrap();
        let kinds: Vec<&str> = replay.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "ipc_request",
                "ipc_response",
                "input",
                "input",
                "input",
                "window",
                "window",
                "window",
                "protocol_load"
            ]
        );
        assert_eq!(replay.skipped, 0);
    }

    #[test]
    fn an_ipc_response_carries_its_duration_and_either_data_or_an_error() {
        let mut r = Recorder::new(header());
        ipc_response(&mut r, 1, 3, 250, Ok(&json!(true))).unwrap();
        let failure = IpcFailure {
            code: "CAP_NO_GRANT".into(),
            message: "no grant".into(),
        };
        ipc_response(&mut r, 2, 4, 90, Err(&failure)).unwrap();
        let replay = read(&r.finish(), Mode::Strict).unwrap();
        assert_eq!(replay.events[0].data["duration_ns"], 250);
        assert_eq!(replay.events[0].data["ok"], true);
        assert_eq!(replay.events[1].data["ok"], false);
        assert_eq!(replay.events[1].data["code"], "CAP_NO_GRANT");
        assert_eq!(replay.events[1].data["duration_ns"], 90);
    }

    #[test]
    fn an_input_value_is_never_recorded_only_its_length() {
        let mut r = Recorder::new(header());
        input(
            &mut r,
            1,
            &Input::Input {
                path: vec![step(0, "textbox", "Card")],
                value_length: 9,
            },
        )
        .unwrap();
        let text = r.finish();
        let replay = read(&text, Mode::Strict).unwrap();
        assert_eq!(replay.events[0].data["value_length"], 9);
        assert!(replay.events[0].data.get("value").is_none());
    }

    #[test]
    fn a_long_accessible_name_is_cut_in_the_path() {
        let mut r = Recorder::new(header());
        let long = "x".repeat(200);
        input(
            &mut r,
            1,
            &Input::Click {
                path: vec![step(0, "button", &long)],
                x: 0,
                y: 0,
            },
        )
        .unwrap();
        let replay = read(&r.finish(), Mode::Strict).unwrap();
        let name = replay.events[0].data["path"][0]["name"].as_str().unwrap();
        assert_eq!(name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_protocol_load_records_the_asset_and_status_and_refuses_bad_names() {
        let mut r = Recorder::new(header());
        protocol_load(&mut r, 1, "app.js", 200).unwrap();
        assert!(protocol_load(&mut r, 2, "../secret", 200).is_err());
        assert!(protocol_load(&mut r, 3, "x.js", 99).is_err());
        let replay = read(&r.finish(), Mode::Strict).unwrap();
        assert_eq!(replay.events.len(), 1);
        assert_eq!(
            replay.events[0].data,
            json!({ "asset": "app.js", "status": 200 })
        );
    }

    #[test]
    fn an_unknown_kind_is_refused_strictly_and_skipped_leniently_beside_the_new_kinds() {
        let text = format!(
            "{}{}\n",
            every_kind(),
            "{\"seq\":9,\"t_ns\":10,\"kind\":\"gesture\",\"data\":{}}"
        );
        let err = read(&text, Mode::Strict).unwrap_err();
        assert!(err.message.contains("gesture"));
        let lenient = read(&text, Mode::Lenient).unwrap();
        assert_eq!(lenient.events.len(), 9);
        assert_eq!(lenient.skipped, 1);
    }
}
