// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Agent surface (spec §10.4, M9): a JSON-RPC session in the MCP style, over newline-delimited
//! messages on stdio.
//!
//! What an agent can do is decided by a list of [`AgentTool`]s. Nothing else is callable, and
//! there is no path to evaluate JavaScript. Each `tools/call` goes through, in order:
//!
//! 1. the kill switch, and the session length limit,
//! 2. the allow-list (only listed tools exist),
//! 3. the rate limit (a [`RateLimiter`] per session),
//! 4. argument validation against the tool's schema,
//! 5. confirmation, for tools the policy marks as needing it,
//! 6. the handler.
//!
//! Every call that reaches the agent is recorded in the action log, including refused ones.
//! The caller decides where that log goes (the audit crate, for example).
//!
//! Gap: tool results are not yet marked as untrusted content for the model. Prompt-injection
//! hygiene is a later step.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tpt_shell_ipc::contract::json_schema;
use tpt_shell_ipc::rate_limit::{RateLimit, RateLimiter};
use tpt_shell_ipc::schema::{validate, Schema};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Runs one tool call. `Err` carries the message the agent sees.
pub type ToolHandler = Box<dyn FnMut(&Value) -> Result<Value, String>>;

/// One command an agent may call.
pub struct AgentTool {
    pub command: String,
    pub description: String,
    pub args: Schema,
    pub handler: ToolHandler,
}

/// Limits and confirmation rules for one session.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Tools in this set need the user's confirmation before each call.
    pub confirm: BTreeSet<String>,
    pub rate: RateLimit,
    /// Calls after this much session time are refused.
    pub max_session: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            confirm: BTreeSet::new(),
            rate: RateLimit::default(),
            max_session: Duration::from_secs(60 * 60),
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
        }
    }

    /// A handle that stops every later tool call when set. Safe to share with a hotkey thread.
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
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "tpt-shell", "version": env!("CARGO_PKG_VERSION") },
            })),
            "tools/list" => Ok(json!({ "tools": self.tool_list() })),
            "tools/call" => Ok(self.call(&params, now, confirm)),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn tool_list(&self) -> Vec<Value> {
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

        if self.kill.load(Ordering::SeqCst) {
            return self.refuse(&name, "the kill switch is on");
        }
        if now > self.policy.max_session {
            return self.refuse(&name, "the session has expired");
        }
        let Some(index) = self.tools.iter().position(|t| t.command == name) else {
            return self.refuse(&name, "no such tool");
        };
        if !self.limiter.allow(now) {
            return self.refuse(&name, "rate limit reached; slow down");
        }
        if let Err(e) = validate(&self.tools[index].args, &args, "arguments") {
            return self.refuse(&name, &format!("invalid arguments: {e}"));
        }
        if self.policy.confirm.contains(&name) && !confirm(&name, &args) {
            return self.refuse(&name, "the user declined");
        }

        match (self.tools[index].handler)(&args) {
            Ok(data) => {
                self.log.push(ActionRecord {
                    command: name,
                    outcome: Outcome::Ran,
                });
                json!({ "content": [{ "type": "text", "text": data.to_string() }], "isError": false })
            }
            Err(message) => {
                self.log.push(ActionRecord {
                    command: name,
                    outcome: Outcome::Failed(message.clone()),
                });
                tool_error(&message)
            }
        }
    }

    fn refuse(&mut self, name: &str, reason: &str) -> Value {
        self.log.push(ActionRecord {
            command: name.to_string(),
            outcome: Outcome::Refused(reason.to_string()),
        });
        tool_error(reason)
    }
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
    fn unknown_methods_and_bad_json_get_json_rpc_errors() {
        let mut s = session(Rc::new(Cell::new(0)), Policy::default());
        let unknown = parse(
            &s.handle_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"resources/read"}"#,
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
