// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The developer overlay in the facade (Windows, debug builds only). Ctrl+Shift+D in an app page
//! shows the overlay window. Release builds never create it, and never add the key script to
//! the pages (`tpt_shell_devtools::enabled` is false there).
//!
//! The overlay window is created hidden when the app is built, and its page is loaded then. Each
//! time it is shown, the shell pushes a fresh snapshot into the page (`window.tptDevtools.update`).
//! The overlay's own page cannot call commands: its messages get a denied reply, because the
//! overlay holds no grants.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{json, Value};
use tpt_shell_core::backend::{
    CloseAction, MessageHandler, WebviewBackend, WindowBackend, WindowSpec,
};
use tpt_shell_core::geometry::LogicalSize;
use tpt_shell_core::Result;
use tpt_shell_devtools::timeline::Model;
use tpt_shell_devtools::{Capability, CommandInfo, Engine, Inspect, IpcEntry, Job, Snapshot};
use tpt_shell_ipc::contract::{json_schema, Contract, Kind};
use tpt_shell_platform_windows::backend::{eval_window, show_window, WindowId, WindowsBackend};
use tpt_shell_replay::{read, read_compressed, Event, Mode};

/// The message the key script posts. It is not a request, so it never reaches the router.
pub const HOTKEY_MESSAGE: &str = "tpt_devtools_hotkey";
/// The key the script listens for, with Ctrl and Shift.
pub const HOTKEY: char = 'd';

const WINDOW_NAME: &str = "tpt-devtools";
/// How many IPC calls the log keeps.
const IPC_LIMIT: usize = 200;
/// The call the overlay page makes to move the timeline cursor. The host answers it with a
/// snapshot that the page shows.
pub const SEEK_COMMAND: &str = "devtools.seek";
/// Installed in the overlay page when it is shown: a seek asks the host through its web message
/// channel, which is the page's only way back to the host.
const SEEK_HOOK_JS: &str = "window.tptDevtools.onSeek = function (position) { window.chrome.webview.postMessage(JSON.stringify({ v: 1, id: 0, cmd: \"devtools.seek\", args: { position: position } })); };";
/// The first bytes of a zstd frame. A recording that starts with them is read as compressed.
#[cfg_attr(not(feature = "devtools"), allow(dead_code))]
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The shell state the overlay shows. The message handlers record into it, and the overlay reads
/// it through [`Inspect`].
///
/// Support: Partial. The jobs are the router's job table, attached in debug builds; the engine
/// version is not reported. The capabilities are the app's grants: a grant without consent is
/// shown as in effect.
pub struct Shell {
    ipc: RefCell<VecDeque<IpcEntry>>,
    audit_path: Option<PathBuf>,
    capabilities: Vec<Capability>,
    contract: Vec<CommandInfo>,
    /// The recording the timeline steps through, when one is loaded.
    timeline: RefCell<Option<Model>>,
    /// The job table of the app's router, once the app attaches it. Empty until then.
    jobs: RefCell<Option<Rc<RefCell<tpt_shell_ipc::jobs::Jobs>>>>,
    /// The latest accessibility check of the page, once one has run.
    a11y: RefCell<Option<tpt_shell_devtools::a11y::Report>>,
    /// The installed WebView2 runtime's version, when the app has read it (see
    /// [`Shell::with_engine_version`]).
    engine_version: Option<String>,
}

impl Shell {
    pub fn new(
        audit_path: Option<PathBuf>,
        grants: &[tpt_shell_capabilities::Grant],
        contract: Option<&Contract>,
    ) -> Self {
        let mut capabilities: Vec<Capability> = grants
            .iter()
            .map(|g| Capability {
                name: format!("{} ({})", g.capability, g.window),
                granted: !g.consent,
            })
            .collect();
        capabilities.sort_by(|a, b| a.name.cmp(&b.name));
        let contract = contract
            .map(|c| {
                c.commands()
                    .map(|cmd| CommandInfo {
                        name: cmd.name.to_string(),
                        kind: match cmd.kind {
                            Kind::Sync => "sync",
                            Kind::Async => "async",
                            Kind::Job => "job",
                        }
                        .to_string(),
                        capability: cmd.capability.map(str::to_string),
                        args_schema: json_schema(&cmd.args),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Shell {
            ipc: RefCell::new(VecDeque::new()),
            audit_path,
            capabilities,
            contract,
            timeline: RefCell::new(None),
            jobs: RefCell::new(None),
            a11y: RefCell::new(None),
            engine_version: None,
        }
    }

    /// Sets the runtime version the Engine panel shows. The app passes what the WebView2 loader
    /// reports, or `None` when the loader cannot say.
    #[must_use]
    pub fn with_engine_version(mut self, version: Option<String>) -> Self {
        self.engine_version = version;
        self
    }

    /// Keeps the result of an accessibility check for the overlay to show.
    ///
    /// Support: Partial. See the devtools accessibility module.
    pub fn set_a11y(&self, report: tpt_shell_devtools::a11y::Report) {
        *self.a11y.borrow_mut() = Some(report);
    }

    /// Gives the overlay the app router's job table, so its jobs panel can list the jobs.
    pub fn attach_jobs(&self, jobs: Rc<RefCell<tpt_shell_ipc::jobs::Jobs>>) {
        *self.jobs.borrow_mut() = Some(jobs);
    }

    /// Records one IPC call for the log, dropping the oldest when the log is full.
    pub fn record(&self, entry: IpcEntry) {
        let mut log = self.ipc.borrow_mut();
        if log.len() == IPC_LIMIT {
            log.pop_front();
        }
        log.push_back(entry);
    }

    /// Loads a recording for the timeline and returns how many events it has. The file is read
    /// strictly unless `mode` says otherwise. On failure the timeline keeps whatever it had.
    ///
    /// Support: Partial. Debug builds on Windows only; the overlay is not created in release.
    #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
    pub fn load_recording(&self, path: &Path, mode: Mode) -> std::result::Result<usize, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let events =
            read_recording(&bytes, mode).map_err(|e| format!("{}: {e}", path.display()))?;
        let count = events.len();
        *self.timeline.borrow_mut() = Some(Model::new(events));
        Ok(count)
    }

    /// The timeline as the overlay reads it, when a recording is loaded.
    ///
    /// Support: Partial. Debug builds on Windows only.
    pub fn timeline_json(&self) -> Option<Value> {
        self.timeline.borrow().as_ref().map(Model::to_json)
    }
}

/// Reads a recording's events from its bytes. A zstd frame is read as compressed, anything else as
/// plain text. The error says what was wrong with the file.
///
/// Support: Partial. Debug builds on Windows only.
#[cfg_attr(not(feature = "devtools"), allow(dead_code))]
pub fn read_recording(bytes: &[u8], mode: Mode) -> std::result::Result<Vec<Event>, String> {
    let replay = if bytes.starts_with(&ZSTD_MAGIC) {
        read_compressed(bytes, mode).map_err(|e| e.to_string())?
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "the recording is not UTF-8 text".to_string())?;
        read(text, mode).map_err(|e| e.to_string())?
    };
    Ok(replay.events)
}

/// The overlay's snapshot: the shell state, with the timeline under `timeline` when a recording
/// is loaded.
///
/// Support: Partial. Debug builds on Windows only.
fn snapshot_json(shell: &Shell) -> Value {
    let mut json = Snapshot::collect(shell).to_json();
    if let (Some(object), Some(timeline)) = (json.as_object_mut(), shell.timeline_json()) {
        object.insert("timeline".into(), timeline);
    }
    json
}

/// The overlay's message handler. A seek from the page moves the timeline cursor and is answered
/// with the new snapshot; any other call is denied.
///
/// Support: Partial. Debug builds on Windows only.
fn overlay_handler(shell: Rc<Shell>) -> MessageHandler {
    Box::new(move |text| {
        let Ok(request) = serde_json::from_str::<Value>(text) else {
            return Some(denied_reply(text));
        };
        if request.get("cmd").and_then(Value::as_str) == Some(SEEK_COMMAND) {
            Some(seek_reply(&shell, &request))
        } else {
            Some(denied_reply(text))
        }
    })
}

/// Moves the timeline cursor to the position the page asked for, and answers with the snapshot.
fn seek_reply(shell: &Shell, request: &Value) -> String {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let position = request
        .get("args")
        .and_then(|args| args.get("position"))
        .and_then(Value::as_u64);
    let outcome = match (position, shell.timeline.borrow_mut().as_mut()) {
        (_, None) => Err("no recording is loaded".to_string()),
        (None, Some(_)) => Err("the seek needs a numeric position".to_string()),
        (Some(position), Some(model)) => model.seek(position as usize),
    };
    match outcome {
        Ok(()) => json!({ "id": id, "ok": true, "result": snapshot_json(shell) }).to_string(),
        Err(message) => json!({
            "id": id,
            "ok": false,
            "error": { "code": "TS0308", "message": message },
        })
        .to_string(),
    }
}

/// The page command that carries an accessibility check's data back to the host.
pub const A11Y_CMD: &str = "tpt.devtools.a11y";

/// Asks the page of `page` to run the accessibility collector and post its data back as a page
/// message with [`A11Y_CMD`]. The overlay opens at once; the report follows when the reply comes.
///
/// Support: Partial. Debug builds on Windows. A page that does not answer leaves the panel at
/// "not checked".
pub fn request_a11y(page: WindowId) -> Result<()> {
    let script = format!(
        "window.chrome.webview.postMessage(JSON.stringify({{ v: 1, id: 0, cmd: \"{A11Y_CMD}\", args: {{ page: {} }} }}));",
        tpt_shell_devtools::a11y::COLLECT_SCRIPT
    );
    eval_window(page, &script)
}

/// The page data in a page message that is an accessibility reply, or `None` for any other message.
///
/// Support: Full.
pub fn a11y_reply(text: &str) -> Option<String> {
    let message: Value = serde_json::from_str(text).ok()?;
    if message.get("cmd").and_then(Value::as_str) != Some(A11Y_CMD) {
        return None;
    }
    message["args"]["page"].as_str().map(str::to_string)
}

impl Inspect for Shell {
    fn a11y(&self) -> Option<tpt_shell_devtools::a11y::Report> {
        self.a11y.borrow().clone()
    }

    fn engine(&self) -> Engine {
        Engine {
            name: "webview2".into(),
            version: self.engine_version.clone(),
            platform: "windows".into(),
        }
    }

    fn ipc_log(&self) -> Vec<IpcEntry> {
        self.ipc.borrow().iter().cloned().collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    fn jobs(&self) -> Vec<Job> {
        let Some(table) = self.jobs.borrow().clone() else {
            return Vec::new();
        };
        let table = table.borrow();
        table
            .list()
            .into_iter()
            .map(|info| Job {
                id: info.id.0,
                name: format!("job {}", info.id.0),
                state: info.state.to_string(),
                progress: info
                    .progress
                    .filter(|(_, total)| *total > 0)
                    .map(|(done, total)| f64::from(done) / f64::from(total)),
            })
            .collect()
    }

    fn audit_jsonl(&self, max: usize) -> String {
        let Some(path) = &self.audit_path else {
            return String::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(max)..].join("\n")
    }

    fn contract(&self) -> Vec<CommandInfo> {
        self.contract.clone()
    }
}

/// Creates the overlay window, hidden, with its page loaded. Returns `None` in a release build.
/// Its close hides it, so the app keeps running when the developer closes it.
///
/// Support: Partial. Debug builds on Windows only; not created in release builds.
pub fn create(backend: &mut WindowsBackend, shell: &Rc<Shell>) -> Result<Option<WindowId>> {
    let Some(page) = tpt_shell_devtools::page_if_enabled(&Snapshot::collect(shell.as_ref())) else {
        return Ok(None);
    };
    let id = backend.create(&WindowSpec {
        name: WINDOW_NAME.into(),
        title: "tpt-shell developer overlay".into(),
        size: LogicalSize {
            width: 900.0,
            height: 700.0,
        },
        allow_navigation: Vec::new(),
    })?;
    backend.set_close_action(id, CloseAction::Hide)?;
    let handler: MessageHandler = overlay_handler(Rc::clone(shell));
    backend.set_message_handler(id, handler)?;
    backend.load_html(id, &page)?;
    Ok(Some(id))
}

/// Adds the Ctrl+Shift+D key script to an app window (debug builds only).
pub fn add_hotkey(backend: &mut WindowsBackend, id: WindowId) -> Result<()> {
    backend.page_key_message(id, HOTKEY, HOTKEY_MESSAGE)
}

/// Shows the overlay with the state as it is now, and connects its seek control to the host.
pub fn open(overlay: WindowId, shell: &Shell) -> Result<()> {
    let json = snapshot_json(shell).to_string();
    show_window(overlay)?;
    eval_window(
        overlay,
        &format!("if (window.tptDevtools) {{ {SEEK_HOOK_JS} window.tptDevtools.update({json}); }}"),
    )
}

/// The reply to a call from the overlay page: denied, because the overlay has no grants.
fn denied_reply(text: &str) -> String {
    let id = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null);
    json!({
        "id": id,
        "ok": false,
        "error": {
            "code": "CAP_DENIED",
            "message": "the developer overlay holds no grants and cannot call commands",
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_from_the_overlay_is_denied_with_the_request_id() {
        let reply: Value =
            serde_json::from_str(&denied_reply(r#"{"v":1,"id":9,"cmd":"x"}"#)).expect("json");
        assert_eq!(reply["id"], json!(9));
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("CAP_DENIED"));
        let junk: Value = serde_json::from_str(&denied_reply("not json")).expect("json");
        assert_eq!(junk["id"], Value::Null);
    }

    #[test]
    fn an_accessibility_reply_is_recognised_by_its_command() {
        let reply =
            json!({ "v": 1, "id": 0, "cmd": A11Y_CMD, "args": { "page": "{\"elements\":[]}" } });
        assert_eq!(
            a11y_reply(&reply.to_string()),
            Some("{\"elements\":[]}".into())
        );
        assert_eq!(
            a11y_reply(r#"{"v":1,"id":2,"cmd":"greet","args":{}}"#),
            None
        );
        assert_eq!(a11y_reply("not json"), None);
    }

    #[test]
    fn the_engine_panel_shows_the_runtime_version_the_app_reads() {
        let shell = Shell::new(None, &[], None);
        assert_eq!(shell.engine().version, None);
        let shell = shell.with_engine_version(Some("155.0.4283.62".into()));
        let engine = shell.engine();
        assert_eq!(engine.name, "webview2");
        assert_eq!(engine.version.as_deref(), Some("155.0.4283.62"));
    }

    #[test]
    fn the_shell_keeps_the_latest_accessibility_report_for_the_snapshot() {
        let shell = Shell::new(None, &[], None);
        assert!(shell.a11y().is_none());
        shell.set_a11y(tpt_shell_devtools::a11y::Report {
            findings: vec![tpt_shell_devtools::a11y::Finding {
                rule: "contrast".into(),
                id: "note".into(),
                message: "contrast is 2.8:1".into(),
            }],
            prefers_reduced_motion: false,
        });
        let json = Snapshot::collect(&shell).to_json();
        assert_eq!(json["a11y"]["findings"][0]["id"], json!("note"));
    }

    #[test]
    fn the_ipc_log_keeps_the_newest_calls() {
        let shell = Shell::new(None, &[], None);
        for i in 0..(IPC_LIMIT as u64 + 5) {
            shell.record(IpcEntry {
                command: format!("c{i}"),
                latency_us: i,
                outcome: "ok".into(),
            });
        }
        let log = shell.ipc_log();
        assert_eq!(log.len(), IPC_LIMIT);
        assert_eq!(log[0].command, "c5");
        assert_eq!(log[IPC_LIMIT - 1].command, format!("c{}", IPC_LIMIT + 4));
    }

    #[test]
    fn the_audit_tail_is_the_last_lines_of_the_log() {
        let dir = std::env::temp_dir().join(format!("tpt-devtools-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("audit.jsonl");
        std::fs::write(&path, "one\ntwo\nthree\n").expect("write");
        let shell = Shell::new(Some(path), &[], None);
        assert_eq!(shell.audit_jsonl(2), "two\nthree");
        assert_eq!(shell.audit_jsonl(10), "one\ntwo\nthree");
        assert_eq!(Shell::new(None, &[], None).audit_jsonl(5), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writes a small recording with one IPC call and one input, and returns its path.
    fn write_recording(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-devtools-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let mut recorder = tpt_shell_replay::Recorder::new(tpt_shell_replay::Header {
            app: "demo".into(),
            platform: "windows".into(),
        });
        recorder
            .record(10, "ipc_request", json!({ "id": 1, "cmd": "greet" }))
            .expect("request");
        recorder
            .record(
                20,
                "ipc_response",
                json!({ "id": 1, "ok": true, "data": "hi", "duration_ns": 10 }),
            )
            .expect("response");
        recorder
            .record(30, "input", json!({ "kind": "click" }))
            .expect("input");
        recorder
            .record(40, "ipc_request", json!({ "id": 2, "cmd": "greet" }))
            .expect("second request");
        let path = dir.join("run.tptreplay");
        std::fs::write(&path, recorder.finish()).expect("write");
        path
    }

    fn reply_of(handler: &mut MessageHandler, text: &str) -> Value {
        serde_json::from_str(&handler(text).expect("a reply")).expect("json")
    }

    #[test]
    fn a_recording_loads_into_the_timeline_with_its_event_count() {
        let path = write_recording("load");
        let shell = Shell::new(None, &[], None);
        assert!(shell.timeline_json().is_none(), "nothing is loaded yet");
        assert_eq!(shell.load_recording(&path, Mode::Strict), Ok(4));
        let timeline = shell.timeline_json().expect("a timeline");
        assert_eq!(timeline["len"], json!(4));
        assert_eq!(timeline["position"], json!(0));
        assert_eq!(timeline["events"][2]["kind"], json!("input"));
        let snapshot = snapshot_json(&shell);
        assert_eq!(snapshot["timeline"]["len"], json!(4));
        assert!(
            snapshot.get("engine").is_some(),
            "the shell state is still there"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn the_jobs_panel_lists_the_routers_jobs_once_attached() {
        use tpt_shell_ipc::jobs::Jobs;
        let shell = Shell::new(None, &[], None);
        assert!(shell.jobs().is_empty(), "no table attached yet");

        let table = Rc::new(RefCell::new(Jobs::new(4)));
        shell.attach_jobs(table.clone());
        let id = table
            .borrow_mut()
            .start(|ctx| {
                ctx.progress(1, 4);
                Ok(json!({}))
            })
            .unwrap();
        // Drain until the job's terminal event has been seen.
        for _ in 0..1000 {
            let events = table.borrow_mut().drain();
            if events.iter().any(|e| e.is_terminal()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let jobs = shell.jobs();
        assert_eq!(jobs.len(), 1, "the finished job is listed: {jobs:?}");
        assert_eq!(jobs[0].id, id.0);
        assert_eq!(jobs[0].name, format!("job {}", id.0));
        assert_eq!(jobs[0].state, "done");
        assert_eq!(jobs[0].progress, Some(0.25));
    }

    #[test]
    fn a_seek_moves_the_cursor_and_the_ipc_state_and_answers_with_a_snapshot() {
        let path = write_recording("seek");
        let shell = Rc::new(Shell::new(None, &[], None));
        shell.load_recording(&path, Mode::Strict).expect("load");
        let mut handler = overlay_handler(Rc::clone(&shell));

        // Before the response, the first call is pending; after it, it has its data.
        let reply = reply_of(
            &mut handler,
            r#"{"v":1,"id":0,"cmd":"devtools.seek","args":{"position":1}}"#,
        );
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["result"]["timeline"]["position"], json!(1));
        assert_eq!(
            reply["result"]["timeline"]["ipc"][0]["state"],
            json!("pending")
        );

        let reply = reply_of(
            &mut handler,
            r#"{"v":1,"id":0,"cmd":"devtools.seek","args":{"position":2}}"#,
        );
        assert_eq!(reply["result"]["timeline"]["position"], json!(2));
        assert_eq!(reply["result"]["timeline"]["ipc"][0]["state"], json!("ok"));
        assert_eq!(reply["result"]["timeline"]["ipc"][0]["data"], json!("hi"));
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn a_seek_outside_the_recording_or_without_one_is_refused_with_a_code() {
        let shell = Rc::new(Shell::new(None, &[], None));
        let mut handler = overlay_handler(Rc::clone(&shell));
        let none = reply_of(
            &mut handler,
            r#"{"v":1,"id":3,"cmd":"devtools.seek","args":{"position":1}}"#,
        );
        assert_eq!(none["ok"], json!(false));
        assert_eq!(none["id"], json!(3));
        assert_eq!(none["error"]["code"], json!("TS0308"));

        let path = write_recording("range");
        shell.load_recording(&path, Mode::Strict).expect("load");
        let past_end = reply_of(
            &mut handler,
            r#"{"v":1,"id":4,"cmd":"devtools.seek","args":{"position":99}}"#,
        );
        assert_eq!(past_end["error"]["code"], json!("TS0308"));
        let no_position = reply_of(
            &mut handler,
            r#"{"v":1,"id":5,"cmd":"devtools.seek","args":{}}"#,
        );
        assert_eq!(no_position["error"]["code"], json!("TS0308"));
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn other_calls_from_the_overlay_are_still_denied() {
        let shell = Rc::new(Shell::new(None, &[], None));
        let mut handler = overlay_handler(shell);
        let reply = reply_of(&mut handler, r#"{"v":1,"id":8,"cmd":"greet"}"#);
        assert_eq!(reply["error"]["code"], json!("CAP_DENIED"));
    }

    #[test]
    fn a_bad_recording_is_refused_with_a_message_and_leaves_the_timeline_empty() {
        let dir = std::env::temp_dir().join(format!("tpt-devtools-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let shell = Shell::new(None, &[], None);

        let missing = dir.join("missing.tptreplay");
        let error = shell
            .load_recording(&missing, Mode::Strict)
            .expect_err("missing file");
        assert!(error.contains("cannot read"), "{error}");

        let not_text = dir.join("binary.tptreplay");
        std::fs::write(&not_text, [0xFF, 0xFE, 0x00]).expect("write");
        assert!(shell.load_recording(&not_text, Mode::Strict).is_err());

        let truncated = dir.join("truncated.tptreplay");
        std::fs::write(&truncated, [0x28, 0xB5, 0x2F, 0xFD, 0x00]).expect("write");
        assert!(shell.load_recording(&truncated, Mode::Strict).is_err());

        let plain = dir.join("plain.tptreplay");
        std::fs::write(&plain, "not a recording\n").expect("write");
        assert!(shell.load_recording(&plain, Mode::Strict).is_err());

        assert!(
            shell.timeline_json().is_none(),
            "no bad file leaves a timeline"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_seek_hook_names_the_same_command_the_handler_answers() {
        assert!(SEEK_HOOK_JS.contains(&format!("cmd: \"{SEEK_COMMAND}\"")));
    }
}
