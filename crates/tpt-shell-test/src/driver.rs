// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The test driver API (spec §10.6). Compiled only with the `test-driver` feature.
//!
//! A test finds elements by role and name, acts on them and reads the page back:
//!
//! ```ignore
//! let mut app = Driver::new(FileChannel::create(&base)?);
//! let button = app.wait_for(&role("button").name("Compress"), Duration::from_secs(5))?;
//! app.click(&button)?;
//! let png = app.screenshot("after-click")?;
//! ```
//!
//! [`Driver`] sends commands through a [`Transport`]. The file channel in [`crate::channel`]
//! is the real transport. A test can supply its own transport, which is how the driver is
//! unit-tested without a running app.
//!
//! Expectations retry automatically. [`Driver::wait_for`] and [`Driver::eventually`] call
//! again every [`POLL_INTERVAL`] while the element is missing, until the timeout.
//!
//! Support: Full for the API on the file channel. Partial: the API is blocking, not `async`,
//! so tests call it directly rather than with `.await`. The app side that answers the
//! commands, and the screenshot capture itself, are routed to the facade and the platform
//! crates.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::a11y::TreeNode;
use tpt_shell_replay::virtual_time::Session;

use crate::channel::{is_artifact_stem, ChannelError, Command, FileChannel};

/// How long a retried expectation waits between attempts.
///
/// Support: Full.
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a single command waits for the app to answer.
///
/// Support: Full.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The error code for an element that is not on the page yet. Only this code is retried.
///
/// Support: Full.
pub const NOT_FOUND: &str = "TS0303";

/// The error code for a locator that matches more than one element. The driver refuses it rather
/// than guess which element a test meant. It is not retried.
///
/// Support: Partial. The code is not in the public registry yet; tpt-shell-errors needs an entry
/// before a release (routed to the errors crate's owner).
pub const AMBIGUOUS: &str = "TS0306";

/// Carries commands to the app and returns its answers. The file channel implements it.
///
/// Support: Full.
pub trait Transport {
    /// Sends a command and returns the app's data.
    ///
    /// Support: Full for the file channel.
    fn call(&mut self, command: &Command) -> Result<Value, ChannelError>;
    /// Reads an artifact the app wrote, such as a screenshot.
    ///
    /// Support: Full for the file channel.
    fn artifact(&self, name: &str) -> Result<Vec<u8>, ChannelError>;
}

impl Transport for FileChannel {
    fn call(&mut self, command: &Command) -> Result<Value, ChannelError> {
        FileChannel::call(self, command, COMMAND_TIMEOUT)
    }

    fn artifact(&self, name: &str) -> Result<Vec<u8>, ChannelError> {
        self.read_artifact(name)
    }
}

/// A way to find an element: its role, and optionally its accessible name. Build one with
/// [`role`].
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locator {
    role: String,
    name: Option<String>,
}

impl Locator {
    /// Narrows the locator to elements with this accessible name.
    ///
    /// Support: Full.
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// The role this locator looks for.
    ///
    /// Support: Full.
    pub fn role_name(&self) -> &str {
        &self.role
    }
}

/// Starts a locator for elements with the given role, such as `button` or `textbox`.
/// Support: Full.
pub fn role(role: &str) -> Locator {
    Locator {
        role: role.to_string(),
        name: None,
    }
}

/// An element the driver has found. It is valid for the page it was found on.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    handle: u64,
}

impl Element {
    /// The app's handle for the element.
    ///
    /// Support: Full.
    pub fn handle(&self) -> u64 {
        self.handle
    }
}

/// The test driver. It is generic over the transport so tests can script the app.
///
/// Support: Full.
pub struct Driver<T: Transport> {
    transport: T,
}

impl<T: Transport> Driver<T> {
    /// Wraps a transport.
    ///
    /// Support: Full.
    pub fn new(transport: T) -> Self {
        Driver { transport }
    }

    /// The transport, for reading its state in a test.
    ///
    /// Support: Full.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Finds the element that matches. Returns a not-found error when none matches, and an
    /// ambiguous error when the app reports more than one match. An answer without a `matches`
    /// count is taken as one match.
    ///
    /// Support: Full. The app must report `matches`; without it the driver cannot refuse an
    /// ambiguous locator, and the app side is routed to the facade.
    pub fn find(&mut self, locator: &Locator) -> Result<Element, ChannelError> {
        let name = locator.name.clone();
        let data = self.transport.call(&Command::Find {
            role: locator.role.clone(),
            name,
        })?;
        let matches = data.get("matches").and_then(Value::as_u64).unwrap_or(1);
        if matches > 1 {
            return Err(ChannelError {
                code: AMBIGUOUS,
                message: format!(
                    "{matches} elements match role {:?}; give the locator a name or a more specific role",
                    locator.role
                ),
            });
        }
        let handle = data
            .get("handle")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol_error("find did not return a handle"))?;
        Ok(Element { handle })
    }

    /// Finds an element, retrying while it is missing, until `timeout`.
    ///
    /// Support: Full.
    pub fn wait_for(
        &mut self,
        locator: &Locator,
        timeout: Duration,
    ) -> Result<Element, ChannelError> {
        self.eventually(timeout, |d| d.find(locator))
    }

    /// Runs `attempt` until it succeeds, or fails with something other than a missing
    /// element, or the timeout passes. Each retry waits [`POLL_INTERVAL`].
    ///
    /// Support: Full.
    pub fn eventually<R>(
        &mut self,
        timeout: Duration,
        mut attempt: impl FnMut(&mut Self) -> Result<R, ChannelError>,
    ) -> Result<R, ChannelError> {
        let deadline = Instant::now() + timeout;
        loop {
            match attempt(self) {
                Ok(value) => return Ok(value),
                Err(e) if e.code == NOT_FOUND && Instant::now() < deadline => {
                    std::thread::sleep(POLL_INTERVAL);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Clicks an element.
    ///
    /// Support: Full.
    pub fn click(&mut self, element: &Element) -> Result<(), ChannelError> {
        self.transport.call(&Command::Click {
            handle: element.handle,
        })?;
        Ok(())
    }

    /// Types text into an element. The text is limited to 1024 characters.
    ///
    /// Support: Full.
    pub fn type_text(&mut self, element: &Element, text: &str) -> Result<(), ChannelError> {
        self.transport.call(&Command::TypeText {
            handle: element.handle,
            text: text.to_string(),
        })?;
        Ok(())
    }

    /// Presses a key by name, such as `Enter` or `Tab`.
    ///
    /// Support: Full.
    pub fn press(&mut self, key: &str) -> Result<(), ChannelError> {
        self.transport.call(&Command::Press {
            key: key.to_string(),
        })?;
        Ok(())
    }

    /// Captures the window as PNG bytes. The name is the artifact's file stem.
    ///
    /// Support: Full on the file channel; the capture is made by the app (CapturePreview on
    /// WebView2, takeSnapshot on WKWebView, and get_snapshot on WebKitGTK are routed to the
    /// platform agents).
    pub fn screenshot(&mut self, name: &str) -> Result<Vec<u8>, ChannelError> {
        if !is_artifact_stem(name) {
            return Err(ChannelError {
                code: "TS0305",
                message: "screenshot name is not valid".into(),
            });
        }
        let data = self.transport.call(&Command::Screenshot {
            name: name.to_string(),
        })?;
        let file = data
            .get("artifact")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error("screenshot did not name an artifact"))?;
        self.transport.artifact(file)
    }

    /// The DOM as the app reports it, as JSON.
    ///
    /// Support: Full on the file channel.
    pub fn dom_snapshot(&mut self) -> Result<Value, ChannelError> {
        self.transport.call(&Command::DomSnapshot)
    }

    /// The native accessibility tree of the window, as role and name pairs.
    ///
    /// Support: Full on the file channel. The native dump is made by the platform (UIA,
    /// NSAccessibility, AT-SPI), which is routed to the platform agents.
    pub fn a11y_tree(&mut self) -> Result<Vec<TreeNode>, ChannelError> {
        let data = self.transport.call(&Command::A11yTree)?;
        let nodes = data
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("a11y_tree did not return nodes"))?;
        nodes
            .iter()
            .map(|n| {
                let role = n
                    .get("role")
                    .and_then(Value::as_str)
                    .ok_or_else(|| protocol_error("a tree node has no role"))?;
                Ok(TreeNode {
                    role: role.to_string(),
                    name: n.get("name").and_then(Value::as_str).map(str::to_string),
                })
            })
            .collect()
    }

    /// Runs a script in the page and returns its JSON result.
    ///
    /// Support: Full on the file channel. The app refuses it unless the build enables test mode.
    pub fn eval(&mut self, script: &str) -> Result<Value, ChannelError> {
        self.transport.call(&Command::Eval {
            script: script.to_string(),
        })
    }
}

/// A transport that records every command and its answer into a replay session, so a test run
/// can be replayed or shown in the time-travel view. Each command becomes an
/// event and each answer an  event with the same id. Recording is skipped
/// silently if the session refuses an event, so the test itself never fails because of it.
///
/// Typed text is recorded as sent. Redact the recording before it leaves the machine
/// (see tpt_shell_replay::report).
///
/// Support: Partial. Virtual devices (spec 10.2) are not wired in: a device event would be a
/// command the app reports, and that path is routed to the device crates.
pub struct Recording<T: Transport> {
    inner: T,
    session: Session,
    next_id: u64,
}

impl<T: Transport> Recording<T> {
    /// Wraps a transport so its traffic goes into .
    ///
    /// Support: Full.
    pub fn new(inner: T, session: Session) -> Self {
        Recording {
            inner,
            session,
            next_id: 0,
        }
    }
}

impl<T: Transport> Transport for Recording<T> {
    fn call(&mut self, command: &Command) -> Result<Value, ChannelError> {
        let id = self.next_id;
        self.next_id += 1;
        let _ = self
            .session
            .record_event("ipc_request", json!({ "id": id, "cmd": command.to_json() }));
        let result = self.inner.call(command);
        let answer = match &result {
            Ok(data) => json!({ "id": id, "ok": true, "data": data }),
            Err(e) => json!({ "id": id, "ok": false, "code": e.code }),
        };
        let _ = self.session.record_event("ipc_response", answer);
        result
    }

    fn artifact(&self, name: &str) -> Result<Vec<u8>, ChannelError> {
        self.inner.artifact(name)
    }
}

fn protocol_error(message: &str) -> ChannelError {
    ChannelError {
        code: "TS0301",
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    /// A transport that answers from a script and records what it was asked.
    struct Scripted {
        answers: VecDeque<Result<Value, ChannelError>>,
        sent: Vec<Command>,
        artifacts: Vec<(String, Vec<u8>)>,
    }

    impl Scripted {
        fn new(answers: Vec<Result<Value, ChannelError>>) -> Self {
            Scripted {
                answers: answers.into(),
                sent: Vec::new(),
                artifacts: Vec::new(),
            }
        }
    }

    impl Transport for Scripted {
        fn call(&mut self, command: &Command) -> Result<Value, ChannelError> {
            self.sent.push(command.clone());
            self.answers.pop_front().unwrap_or_else(|| {
                Err(ChannelError {
                    code: "TS0302",
                    message: "script ran out".into(),
                })
            })
        }

        fn artifact(&self, name: &str) -> Result<Vec<u8>, ChannelError> {
            self.artifacts
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.clone())
                .ok_or(ChannelError {
                    code: "TS0305",
                    message: "no such artifact".into(),
                })
        }
    }

    fn not_found() -> ChannelError {
        ChannelError {
            code: NOT_FOUND,
            message: "no such element".into(),
        }
    }

    #[test]
    fn find_sends_role_and_name_and_returns_a_handle() {
        let mut d = Driver::new(Scripted::new(vec![Ok(json!({ "handle": 4 }))]));
        let el = d.find(&role("button").name("Compress")).unwrap();
        assert_eq!(el.handle(), 4);
        assert_eq!(
            d.transport().sent[0],
            Command::Find {
                role: "button".into(),
                name: Some("Compress".into())
            }
        );
    }

    #[test]
    fn wait_for_retries_while_the_element_is_missing() {
        let mut d = Driver::new(Scripted::new(vec![
            Err(not_found()),
            Err(not_found()),
            Ok(json!({ "handle": 2 })),
        ]));
        let el = d.wait_for(&role("link"), Duration::from_secs(5)).unwrap();
        assert_eq!(el.handle(), 2);
        assert_eq!(d.transport().sent.len(), 3);
    }

    #[test]
    fn wait_for_gives_up_at_the_timeout_with_the_not_found_code() {
        let answers = (0..1000).map(|_| Err(not_found())).collect();
        let mut d = Driver::new(Scripted::new(answers));
        let err = d
            .wait_for(&role("link"), Duration::from_millis(120))
            .unwrap_err();
        assert_eq!(err.code, NOT_FOUND);
        assert!(d.transport().sent.len() >= 2);
    }

    #[test]
    fn other_errors_are_not_retried() {
        let mut d = Driver::new(Scripted::new(vec![Err(ChannelError {
            code: "TS0304",
            message: "not supported here".into(),
        })]));
        let err = d
            .wait_for(&role("link"), Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(err.code, "TS0304");
        assert_eq!(d.transport().sent.len(), 1);
    }

    #[test]
    fn a_bad_role_is_refused_before_anything_is_sent_to_the_app() {
        let mut d = Driver::new(Scripted::new(vec![]));
        let err = d.screenshot("../escape");
        assert!(err.is_err());
        assert!(d.transport().sent.is_empty());
    }

    #[test]
    fn screenshot_reads_the_artifact_the_app_names() {
        let mut t = Scripted::new(vec![Ok(json!({ "artifact": "home.png" }))]);
        t.artifacts.push(("home.png".into(), b"\x89PNG".to_vec()));
        let mut d = Driver::new(t);
        assert_eq!(d.screenshot("home").unwrap(), b"\x89PNG");
    }

    #[test]
    fn a11y_tree_reads_roles_and_optional_names() {
        let mut d = Driver::new(Scripted::new(vec![Ok(json!({
            "nodes": [
                { "role": "button", "name": "Save" },
                { "role": "group" }
            ]
        }))]));
        let nodes = d.a11y_tree().unwrap();
        assert_eq!(nodes[0].name.as_deref(), Some("Save"));
        assert_eq!(nodes[1].name, None);
    }

    #[test]
    fn malformed_answers_are_protocol_errors_not_panics() {
        let mut d = Driver::new(Scripted::new(vec![
            Ok(json!({ "handle": "four" })),
            Ok(json!({ "nodes": [ { "name": "no role" } ] })),
        ]));
        assert_eq!(d.find(&role("button")).unwrap_err().code, "TS0301");
        assert_eq!(d.a11y_tree().unwrap_err().code, "TS0301");
    }

    #[test]
    fn traffic_is_recorded_as_paired_request_and_response_events() {
        use tpt_shell_replay::{read, Header, Mode};
        let header = Header {
            app: "demo".into(),
            platform: "test".into(),
        };
        let session = Session::record(header);
        let scripted = Scripted::new(vec![Ok(json!({ "handle": 5 })), Err(not_found())]);
        let mut d = Driver::new(Recording::new(scripted, session.clone()));
        assert_eq!(d.find(&role("button")).unwrap().handle(), 5);
        assert!(d.find(&role("link")).is_err());
        let text = session.finish().unwrap();
        let replay = read(&text, Mode::Strict).unwrap();
        let kinds: Vec<&str> = replay.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["ipc_request", "ipc_response", "ipc_request", "ipc_response"]
        );
        assert_eq!(replay.events[1].data["id"], 0);
        assert_eq!(replay.events[3].data["code"], NOT_FOUND);
    }

    /// A page the test can change: elements have a role and a name, `hidden` ones appear when the
    /// element that reveals them is clicked, and every click and typed text is recorded.
    struct FakePage {
        elements: Vec<(u64, String, String)>,
        hidden: Vec<(u64, String, String)>,
        reveal_on: Option<u64>,
        clicks: Vec<u64>,
        typed: Vec<(u64, String)>,
    }

    impl FakePage {
        fn new(elements: Vec<(u64, &str, &str)>) -> Self {
            FakePage {
                elements: elements
                    .into_iter()
                    .map(|(h, r, n)| (h, r.into(), n.into()))
                    .collect(),
                hidden: Vec::new(),
                reveal_on: None,
                clicks: Vec::new(),
                typed: Vec::new(),
            }
        }
    }

    impl Transport for FakePage {
        fn call(&mut self, command: &Command) -> Result<Value, ChannelError> {
            match command {
                Command::Find { role, name } => {
                    let found: Vec<&(u64, String, String)> = self
                        .elements
                        .iter()
                        .filter(|(_, r, n)| r == role && name.as_ref().is_none_or(|want| want == n))
                        .collect();
                    match found.first() {
                        None => Err(not_found()),
                        Some((handle, _, _)) => {
                            Ok(json!({ "handle": handle, "matches": found.len() }))
                        }
                    }
                }
                Command::Click { handle } => {
                    self.clicks.push(*handle);
                    if self.reveal_on == Some(*handle) {
                        self.elements.append(&mut self.hidden);
                    }
                    Ok(Value::Null)
                }
                Command::TypeText { handle, text } => {
                    self.typed.push((*handle, text.clone()));
                    Ok(Value::Null)
                }
                _ => Ok(Value::Null),
            }
        }

        fn artifact(&self, _name: &str) -> Result<Vec<u8>, ChannelError> {
            Err(ChannelError {
                code: "TS0305",
                message: "no artifacts on the fake page".into(),
            })
        }
    }

    #[test]
    fn an_ambiguous_locator_is_refused_and_not_retried() {
        let page = FakePage::new(vec![(1, "button", "Save"), (2, "button", "Save")]);
        let mut d = Driver::new(page);
        let err = d
            .wait_for(&role("button").name("Save"), Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(err.code, AMBIGUOUS);
        assert_eq!(d.transport().clicks.len(), 0);
        assert_eq!(
            d.eventually(Duration::ZERO, |d| d.find(&role("button").name("Save")))
                .unwrap_err()
                .code,
            AMBIGUOUS
        );
    }

    #[test]
    fn a_unique_match_with_a_count_of_one_is_accepted() {
        let page = FakePage::new(vec![(7, "textbox", "Name")]);
        let mut d = Driver::new(page);
        assert_eq!(d.find(&role("textbox").name("Name")).unwrap().handle(), 7);
    }

    #[test]
    fn wait_for_finds_a_control_that_appears_after_a_click() {
        let mut page = FakePage::new(vec![(1, "button", "Open")]);
        page.hidden.push((2, "button".into(), "Save".into()));
        page.reveal_on = Some(1);
        let mut d = Driver::new(page);

        // Before the click the control is not on the page, so the wait gives up with the code.
        let early = d
            .wait_for(&role("button").name("Save"), Duration::from_millis(120))
            .unwrap_err();
        assert_eq!(early.code, NOT_FOUND);

        let open = d.find(&role("button").name("Open")).unwrap();
        d.click(&open).unwrap();
        let save = d
            .wait_for(&role("button").name("Save"), Duration::from_secs(5))
            .unwrap();
        d.type_text(&save, "report.pdf").unwrap();

        assert_eq!(d.transport().clicks, vec![1]);
        assert_eq!(d.transport().typed, vec![(2, "report.pdf".to_string())]);
    }

    #[test]
    fn actions_send_the_element_handle() {
        let mut d = Driver::new(Scripted::new(vec![Ok(Value::Null), Ok(Value::Null)]));
        let el = Element { handle: 9 };
        d.click(&el).unwrap();
        d.type_text(&el, "hello").unwrap();
        assert_eq!(d.transport().sent[0], Command::Click { handle: 9 });
        assert_eq!(
            d.transport().sent[1],
            Command::TypeText {
                handle: 9,
                text: "hello".into()
            }
        );
    }
}
