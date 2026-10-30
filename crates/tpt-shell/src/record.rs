// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Recording a running app to a `.tptreplay` file (spec §10.5).
//!
//! The app opens a [`Recording`] when it is built with `App::record`. The IPC calls the pages make,
//! the windows that close and the app assets the pages load go into it, and the file is written
//! when the event loop ends. A recording never changes what the app does: a call that cannot be
//! recorded is logged, and the call still runs.
//!
//! Not recorded yet: window resize and focus, because the backend reports no such events, and page
//! input, because the input shim is not injected into a running window.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tpt_shell_core::{Clock, SystemClock};
use tpt_shell_replay::capture::{self, IpcFailure, WindowEvent};
use tpt_shell_replay::{Header, Recorder};

/// One run's recording: the recorder, the clock its times come from, and the file it is written to
/// when [`Recording::finish`] is called.
///
/// Support: Partial. A running app records on Windows only, and only IPC calls, window closes and
/// protocol loads: resize, focus and page input are not recorded. The recording logic is unit
/// tested.
pub struct Recording {
    path: PathBuf,
    clock: Box<dyn Clock>,
    recorder: RefCell<Option<Recorder>>,
}

impl Recording {
    /// Starts a recording of the app `app`, timed by the system clock, to be written to `path`.
    ///
    /// Support: Full.
    pub fn new(path: impl Into<PathBuf>, app: &str) -> Self {
        Self::with_clock(path, app, Box::new(SystemClock::new()))
    }

    /// Starts a recording timed by `clock`. Tests use this with a clock they control.
    ///
    /// Support: Full.
    pub fn with_clock(path: impl Into<PathBuf>, app: &str, clock: Box<dyn Clock>) -> Self {
        let header = Header {
            app: app.to_string(),
            platform: std::env::consts::OS.to_string(),
        };
        Recording {
            path: path.into(),
            clock,
            recorder: RefCell::new(Some(Recorder::new(header))),
        }
    }

    /// The file this recording is written to.
    ///
    /// Support: Full.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs one IPC call and records it: the request as the page sent it, then the reply, with how
    /// long the call took. `run` makes the reply and the reply is returned unchanged, so recording
    /// never changes what the page sees. A request that does not parse is recorded as the command
    /// `(unparsed)`.
    ///
    /// Support: Full.
    pub fn call(&self, request: &str, run: impl FnOnce() -> String) -> String {
        let (id, cmd, args) = parse_request(request);
        let start = self.now();
        self.write(start, |rec, t| {
            capture::ipc_request(rec, t, id, &cmd, &args)
        });
        let reply = run();
        let end = self.now();
        let duration = end.saturating_sub(start);
        let response: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
        self.write(end, |rec, t| {
            if response["ok"] == Value::Bool(true) {
                capture::ipc_response(rec, t, id, duration, Ok(&response["data"]))
            } else {
                let failure = IpcFailure {
                    code: response["error"]["code"]
                        .as_str()
                        .unwrap_or("UNKNOWN")
                        .to_string(),
                    message: response["error"]["message"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                };
                capture::ipc_response(rec, t, id, duration, Err(&failure))
            }
        });
        reply
    }

    /// Records that a window closed.
    ///
    /// Support: Full.
    pub fn window_closed(&self, window: &str) {
        let event = WindowEvent::Close {
            window: window.to_string(),
        };
        self.write(self.now(), |rec, t| capture::window(rec, t, &event));
    }

    /// Records that a page loaded an app asset, and whether the asset existed (200) or not (404).
    /// A name the reader would refuse, such as one with a slash in it, is not recorded.
    ///
    /// Support: Full.
    pub fn asset_loaded(&self, asset: &str, found: bool) {
        if !tpt_shell_replay::is_asset_name(asset) {
            return;
        }
        let status = if found { 200 } else { 404 };
        self.write(self.now(), |rec, t| {
            capture::protocol_load(rec, t, asset, status)
        });
    }

    /// Writes the recording to its file, compressed. Anything recorded after this is dropped. A
    /// recording that is never finished writes nothing.
    ///
    /// Support: Full.
    pub fn finish(&self) -> Result<(), String> {
        let Some(recorder) = self.recorder.borrow_mut().take() else {
            return Ok(());
        };
        let bytes = recorder.finish_compressed();
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, bytes)
            .map_err(|e| format!("cannot write {}: {e}", self.path.display()))
    }

    fn now(&self) -> u64 {
        nanos(self.clock.monotonic())
    }

    /// Adds one event at time `t`. A failure is logged and the app carries on.
    fn write(&self, t: u64, add: impl FnOnce(&mut Recorder, u64) -> Result<(), String>) {
        let mut slot = self.recorder.borrow_mut();
        let Some(recorder) = slot.as_mut() else {
            return;
        };
        if let Err(e) = add(recorder, t) {
            eprintln!("tpt-shell: cannot record: {e}");
        }
    }
}

/// The envelope's id, command and arguments. A request that does not parse has id 0, the command
/// `(unparsed)` and no arguments.
fn parse_request(request: &str) -> (u64, String, Value) {
    match serde_json::from_str::<Value>(request) {
        Ok(value) => (
            value["id"].as_u64().unwrap_or(0),
            value["cmd"].as_str().unwrap_or("(unparsed)").to_string(),
            value["args"].clone(),
        ),
        Err(_) => (0, "(unparsed)".to_string(), Value::Null),
    }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;
    use tpt_shell_capabilities::{ConsentPrompt, Engine};
    use tpt_shell_ipc::{Caller, Limits, Router};
    use tpt_shell_replay::Mode;

    /// A clock that moves forward by a fixed step on every read, so durations are known.
    struct StepClock(AtomicU64, u64);

    impl Clock for StepClock {
        fn monotonic(&self) -> Duration {
            Duration::from_nanos(self.0.fetch_add(self.1, Ordering::SeqCst))
        }
        fn wall(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH
        }
    }

    struct Refuse;

    impl ConsentPrompt for Refuse {
        fn ask(&mut self, _window: &str, _capability: &str) -> bool {
            false
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tpt-record-{}-{name}.tptreplay",
            std::process::id()
        ))
    }

    fn caller() -> Caller {
        Caller {
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
        }
    }

    #[test]
    fn calls_and_a_close_are_written_and_read_back() {
        let path = temp_file("calls");
        let _ = std::fs::remove_file(&path);
        // An unguarded command, so the unauthorised entry point is the one that runs it; the
        // guarded path refuses a command with no capability, as the refusal test shows.
        let mut router = Router::new();
        router.register("greet", |args| Ok(json!({ "hello": args["name"] })));
        let recording = Recording::with_clock(
            &path,
            "demo",
            Box::new(StepClock(AtomicU64::new(0), 1_000_000)),
        );
        let request = r#"{"v":1,"id":7,"cmd":"greet","args":{"name":"Ada"}}"#;

        let reply = recording.call(request, || {
            router.handle(request.as_bytes(), &Limits::default())
        });
        recording.window_closed("main");
        recording.finish().unwrap();

        assert!(reply.contains("\"ok\":true"), "{reply}");
        let bytes = std::fs::read(&path).unwrap();
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Strict).unwrap();
        let kinds: Vec<&str> = replay.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["ipc_request", "ipc_response", "window"]);
        assert_eq!(replay.header.app, "demo");
        assert_eq!(replay.events[0].data["cmd"], "greet");
        assert_eq!(replay.events[0].data["id"], 7);
        assert_eq!(replay.events[1].data["ok"], true);
        assert_eq!(replay.events[1].data["duration_ns"], 1_000_000);
        assert_eq!(replay.events[1].data["data"]["hello"], "Ada");
        assert_eq!(replay.events[2].data["op"], "close");
        assert_eq!(replay.events[2].data["window"], "main");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_refused_call_is_recorded_as_a_failed_response() {
        let path = temp_file("refused");
        let _ = std::fs::remove_file(&path);
        let router = Router::new();
        let mut engine = Engine::new();
        let caller = caller();
        let recording = Recording::new(&path, "demo");
        let request = r#"{"v":1,"id":3,"cmd":"missing","args":{}}"#;

        recording.call(request, || {
            router.handle_authorized(
                &caller,
                &mut engine,
                &mut Refuse,
                request.as_bytes(),
                &Limits::default(),
            )
        });
        recording.finish().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Strict).unwrap();
        let response = &replay.events[1];
        assert_eq!(response.kind, "ipc_response");
        assert_eq!(response.data["ok"], false);
        assert!(response.data["code"].is_string(), "{:?}", response.data);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn asset_loads_are_recorded_with_their_status_and_nested_names_are_skipped() {
        let path = temp_file("assets");
        let _ = std::fs::remove_file(&path);
        let recording = Recording::new(&path, "demo");

        recording.asset_loaded("index.html", true);
        recording.asset_loaded("js/app.js", true);
        recording.asset_loaded("missing.css", false);
        recording.finish().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Strict).unwrap();
        let loads: Vec<(&str, &Value)> = replay
            .events
            .iter()
            .map(|e| (e.kind.as_str(), &e.data))
            .collect();
        assert_eq!(loads.len(), 2);
        assert_eq!(loads[0].1["asset"], "index.html");
        assert_eq!(loads[0].1["status"], 200);
        assert_eq!(loads[1].1["asset"], "missing.css");
        assert_eq!(loads[1].1["status"], 404);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_recording_that_is_never_finished_writes_nothing() {
        let path = temp_file("unfinished");
        let _ = std::fs::remove_file(&path);
        let recording = Recording::new(&path, "demo");
        recording.window_closed("main");
        drop(recording);
        assert!(!path.exists());
    }

    #[test]
    fn events_after_finish_are_dropped_and_a_second_finish_is_harmless() {
        let path = temp_file("after-finish");
        let _ = std::fs::remove_file(&path);
        let recording = Recording::new(&path, "demo");
        recording.finish().unwrap();
        recording.window_closed("main");
        recording.finish().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Strict).unwrap();
        assert!(replay.events.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
