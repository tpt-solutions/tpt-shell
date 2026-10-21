// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Agent surface (spec §10.4, M9): a JSON-RPC session in the MCP style, over newline-delimited
//! messages on stdio, or over localhost HTTP with a token when the `mcp-http` feature is on.
//!
//! What an agent can do is decided by a list of [`AgentTool`]s and the [`Policy`] (usually built
//! from a manifest profile, see [`profile`]). Nothing else is callable, and there is no path to
//! evaluate JavaScript. Each `tools/call` and `resources/read` goes through, in order:
//!
//! 1. the kill switch, and the session length limit,
//! 2. whether typed commands are allowed at all (the profile's `invoke` action),
//! 3. the allow-list (only listed tools exist),
//! 4. the rate limit (a [`RateLimiter`] per session),
//! 5. argument validation against the tool's schema,
//! 6. the route allow-list, for any `url` or `route` argument (fail closed when empty),
//! 7. confirmation, for tools the policy marks as needing it,
//! 8. the handler.
//!
//! Every call that reaches the agent is recorded in the action log and in the [`record::Recorder`]
//! if one is set, including refused ones. If the recorder fails, the session stops, because an
//! action that cannot be recorded must not run.
//!
//! Results and resource text come from the page, so they are marked as untrusted content (see
//! [`page::mark_untrusted`]). The marker is a convention for the model; the session cannot
//! enforce how the model reads it.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod catalog;
pub mod page;
pub mod profile;
pub mod record;
pub mod resources;

#[cfg(feature = "mcp-http")]
pub mod http;

use page::mark_untrusted;
use profile::RouteRules;
use record::{RecordedAction, Recorder, CODE_RECORD};
use resources::ResourceProvider;
use serde_json::{json, Value};
use tpt_shell_ipc::contract::json_schema;
use tpt_shell_ipc::rate_limit::{RateLimit, RateLimiter};
use tpt_shell_ipc::schema::{validate, Schema};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// A URL argument is outside the profile's route allow-list. Fix: add the route to the profile,
/// or call the tool with a route it allows.
pub const CODE_ROUTE: &str = "TS0112";
/// Typed commands are not allowed for this profile. Fix: add `invoke` to its `actions`.
pub const CODE_INVOKE: &str = "TS0114";

/// Runs one tool call. `Err` carries the message the agent sees.
pub type ToolHandler = Box<dyn FnMut(&Value) -> Result<Value, String>>;

/// One command an agent may call.
pub struct AgentTool {
    pub command: String,
    pub description: String,
    pub args: Schema,
    pub handler: ToolHandler,
}

/// Asks the user before a sensitive action. The platform crates implement this with a native
/// dialog. [`DenyAll`] is the default when no dialog exists: it refuses everything.
pub trait Confirm {
    fn confirm(&mut self, command: &str, args: &Value) -> bool;
}

/// Refuses every confirmation. Used when no dialog is available.
pub struct DenyAll;

impl Confirm for DenyAll {
    fn confirm(&mut self, _command: &str, _args: &Value) -> bool {
        false
    }
}

/// Adapts a [`Confirm`] to the callback that [`Session::handle_line`] takes.
pub fn confirm_with(c: &mut dyn Confirm) -> impl FnMut(&str, &Value) -> bool + '_ {
    move |command, args| c.confirm(command, args)
}

/// Limits and confirmation rules for one session.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Tools in this set need the user's confirmation before each call.
    pub confirm: BTreeSet<String>,
    pub rate: RateLimit,
    /// Calls after this much session time are refused.
    pub max_session: Duration,
    /// URLs an agent may pass to a tool or resource. Empty allows none.
    pub routes: RouteRules,
    /// Whether typed commands (tools) may be called at all.
    pub invoke: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            confirm: BTreeSet::new(),
            rate: RateLimit::default(),
            max_session: Duration::from_secs(60 * 60),
            routes: RouteRules::default(),
            invoke: true,
        }
    }
}

/// What happened to one call, for the action log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ran,
    Failed(String),
    Refused(String),
}

/// One logged call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRecord {
    pub command: String,
    pub outcome: Outcome,
}

/// A session. Feed it one message at a time with [`Session::handle_line`].
pub struct Session {
    tools: Vec<AgentTool>,
    policy: Policy,
    limiter: RateLimiter,
    kill: Arc<AtomicBool>,
    pub log: Vec<ActionRecord>,
    resources: Option<Box<dyn ResourceProvider>>,
    recorder: Option<Box<dyn Recorder>>,
    actor: String,
}

impl Session {
    pub fn new(tools: Vec<AgentTool>, policy: Policy) -> Self {
        let limiter = RateLimiter::new(policy.rate);
        Session {
            tools,
            policy,
            limiter,
            kill: Arc::new(AtomicBool::new(false)),
            log: Vec::new(),
            resources: None,
            recorder: None,
            actor: "agent".into(),
        }
    }

    /// Offers resources (the page tree, the audit tail) over `resources/list` and `resources/read`.
    pub fn with_resources(mut self, provider: Box<dyn ResourceProvider>) -> Self {
        self.resources = Some(provider);
        self
    }

    /// Records every handled call, for example to the audit file.
    pub fn with_recorder(mut self, recorder: Box<dyn Recorder>) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Names the agent in the records, such as `agent:assistant`.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = actor.into();
        self
    }

    /// A handle that stops every later tool call and resource read when set. Safe to share with
    /// a hotkey thread.
    pub fn kill_switch(&self) -> Arc<AtomicBool> {
        self.kill.clone()
    }

    /// Handles one message and returns the response text, or `None` for notifications.
    ///
    /// `now` is the session's own clock, measured from when the session started. `confirm`
    /// is asked for tools that the policy marks, and returns the user's answer.
    pub fn handle_line(
        &mut self,
        line: &str,
        now: Duration,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    ) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                ));
            }
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(json!({}));

        // Notifications carry no id and get no response.
        let id = id?;

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": self.capabilities(),
                "serverInfo": { "name": "tpt-shell", "version": env!("CARGO_PKG_VERSION") },
            })),
            "tools/list" => Ok(json!({ "tools": self.tool_list() })),
            "tools/call" => Ok(self.call(&params, now, confirm)),
            "resources/list" => Ok(json!({ "resources": self.resource_list() })),
            "resources/read" => self.read_resource(&params, now),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn capabilities(&self) -> Value {
        let mut caps = json!({ "tools": {} });
        if self.resources.is_some() {
            caps["resources"] = json!({});
        }
        caps
    }

    fn tool_list(&self) -> Vec<Value> {
        if !self.policy.invoke {
            return Vec::new();
        }
        self.tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.command,
                    "description": t.description,
                    "inputSchema": json_schema(&t.args),
                })
            })
            .collect()
    }

    fn resource_list(&self) -> Vec<Value> {
        self.resources
            .as_ref()
            .map(|p| p.list())
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                json!({
                    "uri": r.uri,
                    "name": r.name,
                    "description": r.description,
                    "mimeType": "text/plain",
                })
            })
            .collect()
    }

    /// The kill switch and the session limit, which apply to every call.
    fn gate(&self, now: Duration) -> Option<&'static str> {
        if self.kill.load(Ordering::SeqCst) {
            Some("the kill switch is on")
        } else if now > self.policy.max_session {
            Some("the session has expired")
        } else {
            None
        }
    }

    fn call(
        &mut self,
        params: &Value,
        now: Duration,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    ) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let started = Instant::now();
        let (outcome, value) = self.run_tool(&name, &args, now, confirm);
        match self.record(&name, &args, &outcome, started) {
            Ok(()) => value,
            Err(e) => tool_error(&format!(
                "{CODE_RECORD}: the action could not be recorded, so the session is stopped: {e}"
            )),
        }
    }

    fn run_tool(
        &mut self,
        name: &str,
        args: &Value,
        now: Duration,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    ) -> (Outcome, Value) {
        if let Some(reason) = self.gate(now) {
            return refused(reason);
        }
        if !self.policy.invoke {
            return refused(&format!(
                "{CODE_INVOKE}: typed commands are not allowed for this profile"
            ));
        }
        let Some(index) = self.tools.iter().position(|t| t.command == name) else {
            return refused("no such tool");
        };
        if !self.limiter.allow(now) {
            return refused("rate limit reached; slow down");
        }
        if let Err(e) = validate(&self.tools[index].args, args, "arguments") {
            return refused(&format!("invalid arguments: {e}"));
        }
        for key in ["url", "route"] {
            if let Some(value) = args.get(key) {
                match value.as_str() {
                    Some(url) if self.policy.routes.allows(url) => {}
                    _ => {
                        return refused(&format!(
                            "{CODE_ROUTE}: the route is outside the allow-list"
                        ))
                    }
                }
            }
        }
        if self.policy.confirm.contains(name) && !confirm(name, args) {
            return refused("the user declined");
        }

        match (self.tools[index].handler)(args) {
            Ok(data) => (
                Outcome::Ran,
                tool_result(&mark_untrusted(&data.to_string())),
            ),
            Err(message) => (
                Outcome::Failed(message.clone()),
                tool_error(&mark_untrusted(&message)),
            ),
        }
    }

    fn read_resource(&mut self, params: &Value, now: Duration) -> Result<Value, (i64, String)> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let args = json!({ "uri": uri });
        let started = Instant::now();
        let command = format!("resource:{uri}");
        let (outcome, result) = match self.gate(now) {
            Some(reason) => (
                Outcome::Refused(reason.into()),
                Err((-32002, reason.into())),
            ),
            None => match self.resources.as_mut() {
                None => (
                    Outcome::Refused("no resources are offered".into()),
                    Err((-32601, "method not found: resources/read".into())),
                ),
                Some(provider) => match provider.read(&uri) {
                    Ok(text) => (
                        Outcome::Ran,
                        Ok(json!({
                            "contents": [{
                                "uri": uri,
                                "mimeType": "text/plain",
                                "text": mark_untrusted(&text),
                            }]
                        })),
                    ),
                    Err(message) => (Outcome::Failed(message.clone()), Err((-32002, message))),
                },
            },
        };
        match self.record(&command, &args, &outcome, started) {
            Ok(()) => result,
            Err(e) => Err((
                -32003,
                format!("{CODE_RECORD}: the read could not be recorded: {e}"),
            )),
        }
    }

    /// Logs the call and gives it to the recorder. A recorder failure stops the session.
    fn record(
        &mut self,
        command: &str,
        args: &Value,
        outcome: &Outcome,
        started: Instant,
    ) -> Result<(), String> {
        self.log.push(ActionRecord {
            command: command.to_string(),
            outcome: outcome.clone(),
        });
        let Some(recorder) = self.recorder.as_mut() else {
            return Ok(());
        };
        let wall_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let action = RecordedAction {
            actor: &self.actor,
            command,
            args,
            outcome,
            duration: started.elapsed(),
            wall_unix_ms,
        };
        recorder.record(&action).inspect_err(|_| {
            self.kill.store(true, Ordering::SeqCst);
        })
    }
}

fn refused(reason: &str) -> (Outcome, Value) {
    (Outcome::Refused(reason.to_string()), tool_error(reason))
}

fn tool_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

/// Runs a session over line-based input and output until the input ends. Each input line is
/// one message; each response is written as one line. `confirm` answers confirmation prompts.
pub fn serve(
    session: &mut Session,
    input: impl std::io::BufRead,
    output: &mut impl std::io::Write,
    start: std::time::Instant,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = session.handle_line(&line, start.elapsed(), confirm) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use tpt_shell_ipc::schema::Field;

    const ARGS: &[Field] = &[Field {
        name: "level",
        schema: Schema::Int { min: 0, max: 10 },
    }];

    const URL_ARGS: &[Field] = &[Field {
        name: "url",
        schema: Schema::Str {
            max_len: 200,
            chars: None,
        },
    }];

    fn tool(name: &str, calls: Rc<Cell<u32>>) -> AgentTool {
        AgentTool {
            command: name.into(),
            description: format!("does {name}"),
            args: Schema::Struct(ARGS),
            handler: Box::new(move |args| {
                calls.set(calls.get() + 1);
                Ok(json!({ "echo": args }))
            }),
        }
    }

    fn call_line(id: u32, name: &str, args: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } })
            .to_string()
    }

    fn never_confirm(_: &str, _: &Value) -> bool {
        panic!("no confirmation expected");
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn session(calls: Rc<Cell<u32>>, policy: Policy) -> Session {
        Session::new(vec![tool("set_volume", calls)], policy)
    }

    #[test]
    fn initialize_reports_the_protocol_and_tools_capability() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        let reply = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        assert!(reply["result"]["capabilities"].get("resources").is_none());
    }

    #[test]
    fn notifications_get_no_response() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        let reply = s.handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            Duration::ZERO,
            &mut never_confirm,
        );
        assert!(reply.is_none());
    }

    #[test]
    fn only_listed_tools_are_offered_with_their_schema() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        let reply = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        let tools = reply["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "set_volume");
        assert_eq!(tools[0]["inputSchema"]["required"], json!(["level"]));
    }

    #[test]
    fn a_listed_tool_runs_and_the_call_is_logged() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default());
        let reply = parse(
            &s.handle_line(
                &call_line(3, "set_volume", json!({ "level": 4 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(calls.get(), 1);
        assert_eq!(
            s.log,
            vec![ActionRecord {
                command: "set_volume".into(),
                outcome: Outcome::Ran
            }]
        );
    }

    #[test]
    fn tool_results_are_marked_as_untrusted() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls, Policy::default());
        let reply = parse(
            &s.handle_line(
                &call_line(3, "set_volume", json!({ "level": 4 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("<untrusted>") && text.ends_with("</untrusted>"));
    }

    #[test]
    fn unlisted_tools_are_refused_and_logged() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default());
        let reply = parse(
            &s.handle_line(
                &call_line(4, "shell_exec", json!({})),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
        assert!(matches!(s.log[0].outcome, Outcome::Refused(_)));
    }

    #[test]
    fn invalid_arguments_never_reach_the_handler() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default());
        let reply = parse(
            &s.handle_line(
                &call_line(5, "set_volume", json!({ "level": 99 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn the_rate_limit_applies_per_session() {
        let calls = Rc::new(Cell::new(0));
        let policy = Policy {
            rate: RateLimit {
                burst: 2,
                per_second: 1,
            },
            ..Policy::default()
        };
        let mut s = session(calls.clone(), policy);
        let mut errors = 0;
        for id in 0..4 {
            let reply = parse(
                &s.handle_line(
                    &call_line(id, "set_volume", json!({ "level": 1 })),
                    Duration::ZERO,
                    &mut never_confirm,
                )
                .unwrap(),
            );
            if reply["result"]["isError"] == true {
                errors += 1;
            }
        }
        assert_eq!(calls.get(), 2);
        assert_eq!(errors, 2);
    }

    #[test]
    fn confirmation_can_decline_a_call() {
        let calls = Rc::new(Cell::new(0));
        let policy = Policy {
            confirm: ["set_volume".to_string()].into_iter().collect(),
            ..Policy::default()
        };
        let mut s = session(calls.clone(), policy);
        let mut asked = Vec::new();
        let reply = parse(
            &s.handle_line(
                &call_line(6, "set_volume", json!({ "level": 2 })),
                Duration::ZERO,
                &mut |name, args| {
                    asked.push((name.to_string(), args.clone()));
                    false
                },
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
        assert_eq!(asked.len(), 1);
    }

    #[test]
    fn the_default_confirmation_refuses_everything() {
        let calls = Rc::new(Cell::new(0));
        let policy = Policy {
            confirm: ["set_volume".to_string()].into_iter().collect(),
            ..Policy::default()
        };
        let mut s = session(calls.clone(), policy);
        let mut dialog = DenyAll;
        let mut ask = confirm_with(&mut dialog);
        let reply = parse(
            &s.handle_line(
                &call_line(6, "set_volume", json!({ "level": 2 })),
                Duration::ZERO,
                &mut ask,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn the_kill_switch_stops_all_later_calls() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default());
        let kill = s.kill_switch();
        kill.store(true, Ordering::SeqCst);
        let reply = parse(
            &s.handle_line(
                &call_line(7, "set_volume", json!({ "level": 1 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn calls_after_the_session_limit_are_refused() {
        let calls = Rc::new(Cell::new(0));
        let policy = Policy {
            max_session: Duration::from_secs(10),
            ..Policy::default()
        };
        let mut s = session(calls.clone(), policy);
        let reply = parse(
            &s.handle_line(
                &call_line(8, "set_volume", json!({ "level": 1 })),
                Duration::from_secs(11),
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_profile_without_invoke_offers_and_runs_no_tools() {
        let calls = Rc::new(Cell::new(0));
        let policy = Policy {
            invoke: false,
            ..Policy::default()
        };
        let mut s = session(calls.clone(), policy);
        let listed = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(listed["result"]["tools"], json!([]));
        let reply = parse(
            &s.handle_line(
                &call_line(3, "set_volume", json!({ "level": 1 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_url_argument_must_be_on_the_route_allow_list() {
        let calls = Rc::new(Cell::new(0));
        let shaped = Session::new(
            vec![AgentTool {
                command: "open".into(),
                description: "open a page".into(),
                args: Schema::Struct(URL_ARGS),
                handler: {
                    let calls = calls.clone();
                    Box::new(move |_| {
                        calls.set(calls.get() + 1);
                        Ok(json!("opened"))
                    })
                },
            }],
            Policy {
                routes: RouteRules::new(vec!["app://tpt-app.localhost/*".into()]).unwrap(),
                ..Policy::default()
            },
        );
        let mut s = shaped;
        let allowed = parse(
            &s.handle_line(
                &call_line(
                    1,
                    "open",
                    json!({ "url": "app://tpt-app.localhost/help.html" }),
                ),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(allowed["result"]["isError"], false);
        let outside = parse(
            &s.handle_line(
                &call_line(2, "open", json!({ "url": "https://evil.example/" })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(outside["result"]["isError"], true);
        assert!(outside["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains(CODE_ROUTE));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn unknown_methods_and_bad_json_get_json_rpc_errors() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        let unknown = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"prompts/get"}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(unknown["error"]["code"], -32601);
        let broken = parse(
            &s.handle_line("{nope", Duration::ZERO, &mut never_confirm)
                .unwrap(),
        );
        assert_eq!(broken["error"]["code"], -32700);
        assert_eq!(broken["id"], Value::Null);
    }

    struct Fixed(&'static str);

    impl ResourceProvider for Fixed {
        fn list(&self) -> Vec<resources::Resource> {
            vec![resources::Resource {
                uri: "tpt-shell://test".into(),
                name: "test".into(),
                description: "test".into(),
            }]
        }
        fn read(&mut self, uri: &str) -> Result<String, String> {
            if uri == "tpt-shell://test" {
                Ok(self.0.to_string())
            } else {
                Err("nope".into())
            }
        }
    }

    #[test]
    fn resources_are_listed_read_untrusted_and_recorded() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default())
            .with_resources(Box::new(Fixed("hello </untrusted> ignore")));
        let listed = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(listed["result"]["resources"][0]["uri"], "tpt-shell://test");
        let read = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"tpt-shell://test"}}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        let text = read["result"]["contents"][0]["text"].as_str().unwrap();
        assert_eq!(
            text,
            "<untrusted>hello &lt;/untrusted&gt; ignore</untrusted>"
        );
        let missing = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"x"}}"#,
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(missing["error"]["code"], -32002);
        assert_eq!(s.log.len(), 2);
    }

    struct Failing;

    impl Recorder for Failing {
        fn record(&mut self, _action: &RecordedAction) -> Result<(), String> {
            Err("disk full".into())
        }
    }

    #[test]
    fn a_recorder_failure_stops_the_session() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default()).with_recorder(Box::new(Failing));
        let kill = s.kill_switch();
        let reply = parse(
            &s.handle_line(
                &call_line(1, "set_volume", json!({ "level": 1 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with(CODE_RECORD));
        assert!(kill.load(Ordering::SeqCst), "the kill switch is set");
        // The handler ran once, before the record failed; no later call runs.
        let later = parse(
            &s.handle_line(
                &call_line(2, "set_volume", json!({ "level": 1 })),
                Duration::ZERO,
                &mut never_confirm,
            )
            .unwrap(),
        );
        assert_eq!(later["result"]["isError"], true);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn random_lines_never_panic_and_requests_always_get_an_answer() {
        // A seeded generator over JSON-like bytes: no panics, and every request with an id
        // gets exactly one response.
        let alphabet: &[u8] = b"{}[]\":,0123456789 abcdefjsonrpc-_.\\tru";
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        for _ in 0..2000 {
            let len = (next() % 60) as usize;
            let line: String = (0..len)
                .map(|_| alphabet[(next() as usize) % alphabet.len()] as char)
                .collect();
            let _ = s.handle_line(&line, Duration::ZERO, &mut never_confirm);
        }
        let reply = s.handle_line(
            r#"{"jsonrpc":"2.0","id":77,"method":"tools/list"}"#,
            Duration::ZERO,
            &mut never_confirm,
        );
        assert!(reply.unwrap().contains("\"id\":77"));
    }

    #[test]
    fn serve_runs_a_scripted_session() {
        let calls = Rc::new(Cell::new(0));
        let mut s = session(calls.clone(), Policy::default());
        let input = format!(
            "{}\n\n{}\n{}\n",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            call_line(2, "set_volume", json!({ "level": 3 })),
        );
        let mut out: Vec<u8> = Vec::new();
        serve(
            &mut s,
            std::io::Cursor::new(input),
            &mut out,
            std::time::Instant::now(),
            &mut never_confirm,
        )
        .unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "one response per request, none for the notification"
        );
        assert_eq!(calls.get(), 1);
    }
}
