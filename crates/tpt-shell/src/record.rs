// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Recording a running app to a `.tptreplay` file (spec §10.5).
//!
//! The app opens a [`Recording`] when it is built with `App::record`. The IPC calls the pages make,
//! the windows that close and the app assets the pages load go into it, and the file is written
//! when the event loop ends. A recording never changes what the app does: a call that cannot be
//! recorded is logged, and the call still runs.
//!
//! Page input (clicks, key presses and input changes, with element paths) is recorded through a
//! listener the app adds to each window before its first page. The same listener reports the
//! window's focus and its size, so those are recorded too, from the page rather than the backend.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tpt_shell_core::{Clock, SystemClock};
use tpt_shell_replay::capture::{self, Input, IpcFailure, PathStep, WindowEvent};
use tpt_shell_replay::{Header, Recorder};

/// One run's recording: the recorder, the clock its times come from, and the file it is written to
/// when [`Recording::finish`] is called.
///
/// Support: Partial. A running app records on Windows only, and records IPC calls, window closes,
/// protocol loads, page input, and window focus and resize as the page reports them. Resize is the
/// page's inner size in CSS pixels, so a recording made on a different scale may not match. The
/// recording logic is unit tested, and the page listener is checked end to end in WebView2
/// (tests/record_input.rs).
pub struct Recording {
    path: PathBuf,
    clock: Box<dyn Clock>,
    recorder: RefCell<Option<Recorder>>,
    paused: std::cell::Cell<bool>,
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
            paused: std::cell::Cell::new(false),
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

    /// Writes the recording so far to its file, compressed, and keeps recording. The dev run uses
    /// it to save a session before a reload.
    ///
    /// Support: Full.
    pub fn save_snapshot(&self) -> Result<(), String> {
        let slot = self.recorder.borrow();
        let Some(recorder) = slot.as_ref() else {
            return Ok(());
        };
        let bytes = recorder.snapshot_compressed();
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, bytes)
            .map_err(|e| format!("cannot write {}: {e}", self.path.display()))
    }

    /// Starts an empty recording with the same header, dropping what was recorded so far. A
    /// recording that was finished stays finished.
    ///
    /// Support: Full.
    pub fn restart(&self) {
        let mut slot = self.recorder.borrow_mut();
        if let Some(recorder) = slot.as_ref() {
            let header = recorder.header().clone();
            *slot = Some(Recorder::new(header));
        }
    }

    /// Pauses or resumes recording. While paused, events are not written. The dev run pauses it
    /// while it replays a session, so the replay is not recorded as new input.
    ///
    /// Support: Full.
    pub fn set_paused(&self, paused: bool) {
        self.paused.set(paused);
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
        if self.paused.get() {
            return;
        }
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
/// The message command a page script uses to report an input event.
pub const INPUT_CMD: &str = "tpt.record.input";

/// The longest page HTML a click or input record carries. A larger page is recorded without it.
pub const MAX_DOM_BYTES: usize = 256 * 1024;

/// The longest element path an input record may carry. A deeper path is dropped with its event.
const MAX_PATH_STEPS: usize = 64;

/// The page script that reports input to the host while a recording runs. It sends one message
/// per click, key press and input change, with the element's path: the role and name of each
/// ancestor and the child index. A key typed into a password field is sent as `[redacted]`.
pub const INPUT_JS: &str = r#"(function () {
  function role(el) { return el.getAttribute('role') || el.tagName.toLowerCase(); }
  function label(el) {
    return el.getAttribute('aria-label') || (el.textContent || '').trim().slice(0, 40);
  }
  function path(el) {
    var steps = [];
    while (el && el.nodeType === 1 && el !== document.documentElement) {
      var parent = el.parentElement;
      var index = parent ? Array.prototype.indexOf.call(parent.children, el) : 0;
      steps.unshift({ index: index, role: role(el), name: label(el) });
      el = parent;
    }
    return steps;
  }
  function send(args) {
    window.chrome.webview.postMessage(JSON.stringify({ v: 1, id: 0, cmd: 'tpt.record.input', args: args }));
  }
  // Every input record goes through one queue, flushed in a timeout, so the records keep the order
  // the events happened in. A click or input record carries the page's HTML after the events of the
  // same task (the flush), so the time-travel view can show the page as those events left it.
  var queue = [];
  var scheduled = false;
  function dom() {
    var html = document.documentElement.outerHTML;
    return html.length <= 262144 ? html : null;
  }
  function flush() {
    scheduled = false;
    queue.splice(0).forEach(function (item) {
      if (item.withDom) item.args.dom = dom();
      send(item.args);
    });
  }
  function enqueue(args, withDom) {
    queue.push({ args: args, withDom: withDom });
    if (!scheduled) {
      scheduled = true;
      setTimeout(flush, 0);
    }
  }
  window.addEventListener('click', function (e) {
    enqueue({ kind: 'click', path: path(e.target), x: e.clientX, y: e.clientY }, true);
  }, true);
  window.addEventListener('keydown', function (e) {
    var secret = e.target && e.target.type === 'password';
    enqueue({ kind: 'key', path: path(e.target), key: secret ? '[redacted]' : e.key }, false);
  }, true);
  window.addEventListener('input', function (e) {
    var target = e.target;
    enqueue({ kind: 'input', path: path(target), value_length: String(target.value || '').length }, true);
  }, true);
  // Window focus and size: these listeners are not capturing, so only the window's own events
  // reach them, not the focus of an element inside the page.
  window.addEventListener('focus', function () {
    enqueue({ kind: 'window-focus' }, false);
  });
  window.addEventListener('resize', function () {
    enqueue({ kind: 'window-resize', width: window.innerWidth, height: window.innerHeight }, false);
  });
})();"#;

/// The page HTML a click or input record carries, if it carries one within the limit.
fn dom_from_args(args: &Value) -> Option<&str> {
    args["dom"]
        .as_str()
        .filter(|html| html.len() <= MAX_DOM_BYTES)
}

/// The input event a page message describes, or `None` when the arguments are not one.
fn input_from_args(args: &Value) -> Option<Input> {
    let steps = args["path"].as_array()?;
    if steps.len() > MAX_PATH_STEPS {
        return None;
    }
    let path = steps
        .iter()
        .map(|s| {
            Some(PathStep {
                index: u32::try_from(s["index"].as_u64()?).ok()?,
                role: s["role"].as_str()?.to_string(),
                name: s["name"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    match args["kind"].as_str()? {
        "click" => Some(Input::Click {
            path,
            x: args["x"].as_f64()?.round() as i64,
            y: args["y"].as_f64()?.round() as i64,
        }),
        "key" => Some(Input::Key {
            path,
            key: args["key"].as_str()?.to_string(),
        }),
        "input" => Some(Input::Input {
            path,
            value_length: args["value_length"].as_u64()?,
        }),
        _ => None,
    }
}

/// The window event a page message describes (focus or resize), or `None`.
fn window_event_from_args(window: &str, args: &Value) -> Option<WindowEvent> {
    match args["kind"].as_str()? {
        "window-focus" => Some(WindowEvent::Focus {
            window: window.to_string(),
        }),
        "window-resize" => Some(WindowEvent::Resize {
            window: window.to_string(),
            width: u32::try_from(args["width"].as_u64()?).ok()?,
            height: u32::try_from(args["height"].as_u64()?).ok()?,
        }),
        _ => None,
    }
}

impl Recording {
    /// Records an input or window event the page script reported, if `text` is one. `window` is
    /// the window the message came from. Returns whether it was, so the host keeps it away from
    /// the router. A malformed input record is dropped, and still counts as one, since it is the
    /// page's message and not a command.
    ///
    /// Support: Full.
    pub fn page_input(&self, window: &str, text: &str) -> bool {
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        if message["cmd"].as_str() != Some(INPUT_CMD) {
            return false;
        }
        let args = &message["args"];
        if let Some(input) = input_from_args(args) {
            let dom = dom_from_args(args).map(str::to_string);
            self.write(self.now(), |rec, t| {
                capture::input_with_dom(rec, t, &input, dom.as_deref())
            });
        } else if let Some(event) = window_event_from_args(window, args) {
            self.write(self.now(), |rec, t| capture::window(rec, t, &event));
        }
        true
    }
}

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

#[cfg(test)]
mod input_tests {
    use super::*;
    use serde_json::json;
    use tpt_shell_replay::Mode;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tpt-record-input-{}-{name}.tptreplay",
            std::process::id()
        ))
    }

    /// Records the messages, finishes the file, and reads back the input events it holds.
    fn recorded(name: &str, messages: &[Value]) -> (Vec<Value>, Vec<bool>) {
        let path = temp(name);
        let recording = Recording::new(&path, "demo");
        let consumed: Vec<bool> = messages
            .iter()
            .map(|m| recording.page_input("main", &m.to_string()))
            .collect();
        recording.finish().expect("finish");
        let bytes = std::fs::read(&path).expect("read");
        let _ = std::fs::remove_file(&path);
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Lenient).expect("read back");
        let events = replay
            .events
            .into_iter()
            .filter(|e| e.kind == "input")
            .map(|e| e.data)
            .collect();
        (events, consumed)
    }

    fn message(args: Value) -> Value {
        json!({ "v": 1, "id": 0, "cmd": INPUT_CMD, "args": args })
    }

    #[test]
    fn window_focus_and_resize_are_recorded_for_the_window_that_sent_them() {
        let path = temp("window-events");
        let _ = std::fs::remove_file(&path);
        let recording = Recording::new(&path, "demo");
        let focus = message(json!({ "kind": "window-focus" }));
        let resize = message(json!({ "kind": "window-resize", "width": 800, "height": 600 }));
        assert!(recording.page_input("settings", &focus.to_string()));
        assert!(recording.page_input("settings", &resize.to_string()));
        recording.finish().expect("finish");
        let bytes = std::fs::read(&path).expect("read");
        let _ = std::fs::remove_file(&path);
        let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Strict).expect("read back");
        let window: Vec<_> = replay
            .events
            .iter()
            .filter(|e| e.kind == "window")
            .collect();
        assert_eq!(window.len(), 2, "{:?}", replay.events);
        assert_eq!(window[0].data["op"], json!("focus"));
        assert_eq!(window[0].data["window"], json!("settings"));
        assert_eq!(window[1].data["op"], json!("resize"));
        assert_eq!(window[1].data["width"], json!(800));
        assert_eq!(window[1].data["height"], json!(600));
    }

    #[test]
    fn a_resize_with_a_negative_size_is_consumed_and_not_recorded() {
        let (events, consumed) = recorded(
            "bad-resize",
            &[message(
                json!({ "kind": "window-resize", "width": -1, "height": 600 }),
            )],
        );
        assert_eq!(consumed, [true]);
        assert!(events.is_empty());
    }

    #[test]
    fn each_kind_of_page_input_is_recorded_with_its_path() {
        let path = json!([{ "index": 1, "role": "button", "name": "Save" }]);
        let (events, consumed) = recorded(
            "kinds",
            &[
                message(json!({ "kind": "click", "path": path, "x": 10.4, "y": 20.6 })),
                message(json!({ "kind": "key", "path": path, "key": "Enter" })),
                message(json!({ "kind": "input", "path": path, "value_length": 3 })),
            ],
        );
        assert_eq!(consumed, [true, true, true]);
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(events[0]["path"][0]["name"], json!("Save"));
        assert!(events.iter().any(|e| e["key"] == json!("Enter")));
        assert!(events.iter().any(|e| e["value_length"] == json!(3)));
        assert!(events
            .iter()
            .any(|e| e["x"] == json!(10) && e["y"] == json!(21)));
    }

    #[test]
    fn a_redacted_key_is_recorded_as_redacted() {
        let (events, _) = recorded(
            "redacted",
            &[message(json!({
                "kind": "key",
                "path": [{ "index": 0, "role": "input", "name": "" }],
                "key": "[redacted]",
            }))],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["key"], json!("[redacted]"));
    }

    #[test]
    fn a_malformed_input_is_consumed_but_not_recorded() {
        let (events, consumed) = recorded(
            "malformed",
            &[
                message(json!({ "kind": "click", "path": [], "x": "left", "y": 1 })),
                message(json!({ "kind": "wheel", "path": [] })),
            ],
        );
        assert_eq!(consumed, [true, true]);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_path_deeper_than_the_limit_is_dropped() {
        let deep: Vec<Value> = (0..=MAX_PATH_STEPS)
            .map(|i| json!({ "index": 0, "role": "div", "name": format!("{i}") }))
            .collect();
        let (events, _) = recorded(
            "deep",
            &[message(json!({ "kind": "key", "path": deep, "key": "a" }))],
        );
        assert!(events.is_empty());
    }

    #[test]
    fn other_messages_are_not_input_and_go_on_to_the_router() {
        let (events, consumed) = recorded(
            "other",
            &[
                json!({ "v": 1, "id": 4, "cmd": "read", "args": {} }),
                json!("not json"),
            ],
        );
        assert_eq!(consumed, [false, false]);
        assert!(events.is_empty());
    }
}
